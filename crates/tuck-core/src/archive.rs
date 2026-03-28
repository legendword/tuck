use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::checksum;
use crate::copy;
use crate::drive::{self, archive_path_on_drive, DriveInfo};
use crate::error::{IoContext, TuckError, TuckResult};
use crate::manifest::{ArchiveEntry, FileChecksum, Manifest};
use crate::pending::{PendingKind, PendingOperation};
use crate::progress::Progress;

/// Information about a planned add operation.
#[derive(Debug)]
pub struct AddPlan {
    pub original_path: PathBuf,
    pub archive_path: PathBuf,
    pub drive_name: String,
    pub drive_root: PathBuf,
    pub is_directory: bool,
    pub size_bytes: u64,
}

/// Plan an add operation: validate paths, compute sizes, check for conflicts.
/// If `force` is true, allow replacing an existing archive entry.
pub fn plan_add(path: &Path, drive: &DriveInfo, force: bool) -> TuckResult<AddPlan> {
    // Canonicalize the source path
    let original_path = path
        .canonicalize()
        .map_err(|_| TuckError::PathNotFound(path.to_path_buf()))?;

    if !original_path.exists() {
        return Err(TuckError::PathNotFound(original_path));
    }

    let archive_path = archive_path_on_drive(&drive.root_path, &original_path);

    // Check if already archived in manifest
    let manifest = Manifest::load(&drive.root_path)?;
    if manifest.find_entry(&original_path).is_some() {
        if !force {
            return Err(TuckError::AlreadyExists(original_path));
        }
    }

    let is_directory = original_path.is_dir();
    let size_bytes = copy::path_size(&original_path)?;

    // Check that the drive has enough free space
    drive::check_space(&drive.mount_path, size_bytes)?;

    Ok(AddPlan {
        original_path,
        archive_path,
        drive_name: drive.name.clone(),
        drive_root: drive.root_path.clone(),
        is_directory,
        size_bytes,
    })
}

/// Execute an add: copy files to drive, verify checksums, update manifest.
/// Returns the checksums of the archived files.
pub fn execute_add(
    plan: &AddPlan,
    progress: Option<&dyn Progress>,
) -> TuckResult<Vec<FileChecksum>> {
    // Ensure root directory exists (needed when using --prefix)
    std::fs::create_dir_all(&plan.drive_root).io_context(&plan.drive_root)?;

    // Remove existing archive if replacing
    if plan.archive_path.exists() {
        copy::remove_path(&plan.archive_path)?;
    }
    // Remove existing manifest entry if replacing
    let mut manifest = Manifest::load(&plan.drive_root)?;
    if manifest.find_entry(&plan.original_path).is_some() {
        manifest.remove_entry(&plan.original_path)?;
        manifest.save(&plan.drive_root)?;
    }

    // Write pending marker before starting
    let pending = PendingOperation {
        kind: PendingKind::Add,
        original_path: plan.original_path.clone(),
        archive_path: plan.archive_path.clone(),
        started_at: Utc::now(),
    };
    PendingOperation::write(&plan.drive_root, &pending)?;

    // Step 1: Hash source files before copy
    if let Some(p) = progress {
        p.start_phase("Hashing source", plan.size_bytes);
    }
    let source_checksums = checksum::hash_path(&plan.original_path, progress)?;
    if let Some(p) = progress {
        p.finish_phase();
    }

    // Step 2: Copy to drive
    if let Some(p) = progress {
        p.start_phase("Copying to drive", plan.size_bytes);
    }
    copy::copy_recursive(&plan.original_path, &plan.archive_path, progress)?;
    if let Some(p) = progress {
        p.finish_phase();
    }

    // Step 3: Hash destination files after copy
    if let Some(p) = progress {
        p.start_phase("Verifying copy", plan.size_bytes);
    }
    let dest_checksums = checksum::hash_path(&plan.archive_path, progress)?;
    if let Some(p) = progress {
        p.finish_phase();
    }

    // Step 4: Compare checksums
    if source_checksums.len() != dest_checksums.len() {
        return Err(TuckError::ChecksumMismatch {
            path: plan.archive_path.clone(),
            expected: format!("{} files", source_checksums.len()),
            actual: format!("{} files", dest_checksums.len()),
        });
    }
    for (src, dst) in source_checksums.iter().zip(dest_checksums.iter()) {
        if src.hash != dst.hash {
            return Err(TuckError::ChecksumMismatch {
                path: plan.archive_path.join(&src.relative_path),
                expected: src.hash.clone(),
                actual: dst.hash.clone(),
            });
        }
    }

    // Step 5: Update manifest with destination checksums
    let entry = ArchiveEntry {
        original_path: plan.original_path.clone(),
        is_directory: plan.is_directory,
        archived_at: Utc::now(),
        size_bytes: plan.size_bytes,
        checksums: dest_checksums.clone(),
        drive_name: plan.drive_name.clone(),
    };

    let mut manifest = Manifest::load(&plan.drive_root)?;
    manifest.add_entry(entry)?;
    manifest.save(&plan.drive_root)?;

    // Clear pending marker — operation completed successfully
    PendingOperation::clear(&plan.drive_root)?;

    Ok(dest_checksums)
}

/// Delete the local copy of an archived path.
pub fn delete_local(original_path: &Path) -> TuckResult<()> {
    copy::remove_path(original_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fake_drive(tmp: &TempDir) -> DriveInfo {
        DriveInfo {
            name: "TestDrive".to_string(),
            mount_path: tmp.path().to_path_buf(),
            root_path: tmp.path().to_path_buf(),
        }
    }

    #[test]
    fn test_plan_add_already_exists_without_force() {
        let drive_dir = TempDir::new().unwrap();
        let source_dir = TempDir::new().unwrap();
        let file = source_dir.path().join("test.txt");
        fs::write(&file, "hello").unwrap();

        let drive = fake_drive(&drive_dir);

        // First add succeeds
        let plan = plan_add(&file, &drive, false).unwrap();
        execute_add(&plan, None).unwrap();

        // Second add without force fails
        let err = plan_add(&file, &drive, false).unwrap_err();
        assert!(matches!(err, TuckError::AlreadyExists(_)));
    }

    #[test]
    fn test_plan_add_already_exists_with_force() {
        let drive_dir = TempDir::new().unwrap();
        let source_dir = TempDir::new().unwrap();
        let file = source_dir.path().join("test.txt");
        fs::write(&file, "hello").unwrap();

        let drive = fake_drive(&drive_dir);

        // First add
        let plan = plan_add(&file, &drive, false).unwrap();
        execute_add(&plan, None).unwrap();

        // Second add with force succeeds
        let plan = plan_add(&file, &drive, true).unwrap();
        assert!(plan.original_path.exists());
    }

    #[test]
    fn test_force_add_replaces_archive_content() {
        let drive_dir = TempDir::new().unwrap();
        let source_dir = TempDir::new().unwrap();
        let file = source_dir.path().join("test.txt");
        fs::write(&file, "version 1").unwrap();

        let drive = fake_drive(&drive_dir);

        // First add
        let plan = plan_add(&file, &drive, false).unwrap();
        let checksums1 = execute_add(&plan, None).unwrap();

        // Modify local file
        fs::write(&file, "version 2").unwrap();

        // Force add replaces archive
        let plan = plan_add(&file, &drive, true).unwrap();
        let checksums2 = execute_add(&plan, None).unwrap();

        // Checksums should differ
        assert_ne!(checksums1[0].hash, checksums2[0].hash);

        // Manifest should have exactly one entry
        let manifest = Manifest::load(&drive.root_path).unwrap();
        assert_eq!(manifest.entries.len(), 1);

        // Archived file should have new content
        let archived = fs::read_to_string(&plan.archive_path).unwrap();
        assert_eq!(archived, "version 2");
    }
}

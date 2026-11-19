use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::checksum;
use crate::copy;
use crate::drive::{self, safe_archive_path, DriveInfo};
use crate::error::{IoContext, TuckError, TuckResult};
use crate::manifest::{ArchiveEntry, FileChecksum, Manifest};
use crate::pending::{PendingKind, PendingOperation, RestoreJournal};
use crate::progress::Progress;
use crate::safety;

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

    // The tuck root must be on the drive, and the archive location must stay
    // inside it. The drive's contents are untrusted: a symlink planted there
    // (say `<drive>/Users -> /Users`) would otherwise make the "archive path"
    // resolve back onto the local disk.
    if drive.root_path != drive.mount_path {
        safety::ensure_contained(&drive.mount_path, &drive.root_path)?;
    }
    let archive_path = safe_archive_path(&drive.root_path, &original_path)?;
    ensure_source_excludes_root(&original_path, drive)?;

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

/// Refuse to archive a directory that contains the tuck root itself (for
/// example `/`, `/Volumes`, or the prefix folder): the copy would recurse into
/// its own destination, and the local delete afterwards would take the
/// archive with it.
fn ensure_source_excludes_root(original_path: &Path, drive: &DriveInfo) -> TuckResult<()> {
    let mount = drive
        .mount_path
        .canonicalize()
        .unwrap_or_else(|_| drive.mount_path.clone());
    let root = match drive.root_path.strip_prefix(&drive.mount_path) {
        Ok(rel) => mount.join(rel),
        Err(_) => drive.root_path.clone(),
    };
    if root.starts_with(original_path) {
        return Err(TuckError::UnsafePath {
            path: original_path.to_path_buf(),
            reason: format!("it contains the tuck folder {}", root.display()),
        });
    }
    Ok(())
}

/// Execute an add: copy files to drive, verify checksums, update manifest.
/// Returns the checksums of the archived files.
pub fn execute_add(
    plan: &AddPlan,
    progress: Option<&dyn Progress>,
) -> TuckResult<Vec<FileChecksum>> {
    execute_add_with_journal(plan, progress, &RestoreJournal::default_location())
}

/// [`execute_add`] with an explicit restore-journal location.
pub fn execute_add_with_journal(
    plan: &AddPlan,
    progress: Option<&dyn Progress>,
    journal: &RestoreJournal,
) -> TuckResult<Vec<FileChecksum>> {
    // Ensure root directory exists (needed when using --prefix)
    std::fs::create_dir_all(&plan.drive_root).io_context(&plan.drive_root)?;

    // Re-check the destination right before touching it. The step below
    // deletes whatever is at `archive_path`, so it must be exactly the archive
    // location for this source and must not lead off the drive.
    let expected = safe_archive_path(&plan.drive_root, &plan.original_path)?;
    if expected != plan.archive_path {
        return Err(TuckError::UnsafePath {
            path: plan.archive_path.clone(),
            reason: format!(
                "it is not the archive location for {}",
                plan.original_path.display()
            ),
        });
    }

    // Remove existing archive if replacing
    if std::fs::symlink_metadata(&plan.archive_path).is_ok() {
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
    // This marker replaces any earlier one on the drive, so a leftover local
    // record of an interrupted restore no longer corresponds to anything.
    let _ = journal.forget_drive(&plan.drive_root);

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
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn fake_drive(tmp: &TempDir) -> DriveInfo {
        DriveInfo {
            name: "TestDrive".to_string(),
            mount_path: tmp.path().to_path_buf(),
            root_path: tmp.path().to_path_buf(),
        }
    }

    /// Run an add with the restore journal kept inside the fake drive's temp dir.
    fn execute_add(plan: &AddPlan, progress: Option<&dyn Progress>) -> TuckResult<Vec<FileChecksum>> {
        let journal = RestoreJournal::at(plan.drive_root.join(".test-journal.json"));
        execute_add_with_journal(plan, progress, &journal)
    }

    /// A source directory with one file; returns its canonical path.
    fn source_dir(tmp: &TempDir) -> PathBuf {
        let dir = tmp.path().canonicalize().unwrap().join("project");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("a.txt"), "precious").unwrap();
        dir
    }

    #[test]
    fn test_symlink_on_drive_cannot_redirect_add_onto_the_source() {
        let drive_dir = TempDir::new().unwrap();
        let source_tmp = TempDir::new().unwrap();
        let source = source_dir(&source_tmp);

        // <drive>/<first component> -> /<first component>: the archive path
        // would resolve to the source itself, which execute_add used to delete
        // as an "existing archive".
        let first = source.components().nth(1).unwrap().as_os_str().to_owned();
        symlink(Path::new("/").join(&first), drive_dir.path().join(&first)).unwrap();

        let drive = fake_drive(&drive_dir);
        let err = plan_add(&source, &drive, false).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));

        // Even a plan built before the symlink appeared is refused.
        let plan = AddPlan {
            original_path: source.clone(),
            archive_path: drive::archive_path_on_drive(&drive.root_path, &source),
            drive_name: drive.name.clone(),
            drive_root: drive.root_path.clone(),
            is_directory: true,
            size_bytes: 8,
        };
        let err = execute_add(&plan, None).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));

        assert_eq!(fs::read_to_string(source.join("a.txt")).unwrap(), "precious");
    }

    #[test]
    fn test_execute_add_refuses_mismatched_archive_path() {
        let drive_dir = TempDir::new().unwrap();
        let source_tmp = TempDir::new().unwrap();
        let source = source_dir(&source_tmp);
        let victim_tmp = TempDir::new().unwrap();
        let victim = source_dir(&victim_tmp);

        let drive = fake_drive(&drive_dir);
        let mut plan = plan_add(&source, &drive, false).unwrap();
        plan.archive_path = victim.clone();

        let err = execute_add(&plan, None).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));
        assert!(victim.join("a.txt").exists());
        assert!(source.join("a.txt").exists());
    }

    #[test]
    fn test_add_does_not_write_metadata_through_planted_symlinks() {
        let drive_dir = TempDir::new().unwrap();
        let source_tmp = TempDir::new().unwrap();
        let source = source_dir(&source_tmp);
        let victim_tmp = TempDir::new().unwrap();
        let rc = victim_tmp.path().join(".zshrc");
        fs::write(&rc, "export PATH=...").unwrap();
        let keys = victim_tmp.path().join("authorized_keys");

        symlink(&rc, drive_dir.path().join(".tuck-manifest.json.tmp")).unwrap();
        symlink(&keys, drive_dir.path().join(".tuck-pending.json")).unwrap();
        symlink(&keys, drive_dir.path().join(".tuck-pending.json.tmp")).unwrap();

        let drive = fake_drive(&drive_dir);
        let plan = plan_add(&source, &drive, false).unwrap();
        execute_add(&plan, None).unwrap();

        assert_eq!(fs::read_to_string(&rc).unwrap(), "export PATH=...");
        assert!(fs::symlink_metadata(&keys).is_err());
        assert_eq!(Manifest::load(&drive.root_path).unwrap().entries.len(), 1);
    }

    #[test]
    fn test_plan_add_refuses_source_containing_the_tuck_root() {
        let outer = TempDir::new().unwrap();
        let outer_path = outer.path().canonicalize().unwrap();
        let mount = outer_path.join("drive");
        fs::create_dir(&mount).unwrap();
        let drive = DriveInfo {
            name: "TestDrive".to_string(),
            mount_path: mount.clone(),
            root_path: mount.clone(),
        };

        for source in [outer_path.as_path(), mount.as_path()] {
            let err = plan_add(source, &drive, false).unwrap_err();
            assert!(matches!(err, TuckError::UnsafePath { .. }), "{source:?}");
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

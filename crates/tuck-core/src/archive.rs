use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::checksum;
use crate::copy;
use crate::drive::{archive_path_on_drive, DriveInfo};
use crate::error::{TuckError, TuckResult};
use crate::manifest::{ArchiveEntry, FileChecksum, Manifest};

/// Information about a planned add operation.
#[derive(Debug)]
pub struct AddPlan {
    pub original_path: PathBuf,
    pub archive_path: PathBuf,
    pub drive_name: String,
    pub drive_mount: PathBuf,
    pub is_directory: bool,
    pub size_bytes: u64,
}

/// Plan an add operation: validate paths, compute sizes, check for conflicts.
pub fn plan_add(path: &Path, drive: &DriveInfo) -> TuckResult<AddPlan> {
    // Canonicalize the source path
    let original_path = path
        .canonicalize()
        .map_err(|_| TuckError::PathNotFound(path.to_path_buf()))?;

    if !original_path.exists() {
        return Err(TuckError::PathNotFound(original_path));
    }

    let archive_path = archive_path_on_drive(&drive.mount_path, &original_path);

    // Check if already archived in manifest
    let manifest = Manifest::load(&drive.mount_path)?;
    if manifest.find_entry(&original_path).is_some() {
        return Err(TuckError::AlreadyExists(original_path));
    }

    let is_directory = original_path.is_dir();
    let size_bytes = copy::path_size(&original_path)?;

    Ok(AddPlan {
        original_path,
        archive_path,
        drive_name: drive.name.clone(),
        drive_mount: drive.mount_path.clone(),
        is_directory,
        size_bytes,
    })
}

/// Execute an add: copy files to drive, verify checksums, update manifest.
/// Returns the checksums of the archived files.
pub fn execute_add(plan: &AddPlan) -> TuckResult<Vec<FileChecksum>> {
    // Step 1: Hash source files before copy
    let source_checksums = checksum::hash_path(&plan.original_path)?;

    // Step 2: Copy to drive
    copy::copy_recursive(&plan.original_path, &plan.archive_path)?;

    // Step 3: Hash destination files after copy
    let dest_checksums = checksum::hash_path(&plan.archive_path)?;

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

    let mut manifest = Manifest::load(&plan.drive_mount)?;
    manifest.add_entry(entry)?;
    manifest.save(&plan.drive_mount)?;

    Ok(dest_checksums)
}

/// Delete the local copy of an archived path.
pub fn delete_local(original_path: &Path) -> TuckResult<()> {
    copy::remove_path(original_path)
}

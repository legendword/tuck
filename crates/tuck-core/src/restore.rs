use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::checksum;
use crate::copy;
use crate::drive::{self, archive_path_on_drive, DriveInfo};
use crate::error::{TuckError, TuckResult};
use crate::manifest::{ArchiveEntry, Manifest};
use crate::pending::{PendingKind, PendingOperation};
use crate::progress::Progress;

/// Information about a planned restore operation.
#[derive(Debug)]
pub struct RestorePlan {
    pub original_path: PathBuf,
    pub archive_path: PathBuf,
    pub drive_root: PathBuf,
    pub entry: ArchiveEntry,
    pub local_exists: bool,
}

/// Plan a restore: find the entry in the manifest, validate paths.
pub fn plan_restore(path: &Path, drive: &DriveInfo) -> TuckResult<RestorePlan> {
    // Try to canonicalize, but if the local path doesn't exist that's expected
    let original_path = if path.exists() {
        path.canonicalize()
            .map_err(|_| TuckError::PathNotFound(path.to_path_buf()))?
    } else {
        // For paths that don't exist locally, use the absolute path as-is
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| TuckError::Io {
                    source: e,
                    path: path.to_path_buf(),
                })?
                .join(path)
        }
    };

    let manifest = Manifest::load(&drive.root_path)?;
    let entry = manifest
        .find_entry(&original_path)
        .ok_or_else(|| TuckError::NotArchived(original_path.clone()))?
        .clone();

    let archive_path = archive_path_on_drive(&drive.root_path, &original_path);

    if !archive_path.exists() {
        return Err(TuckError::PathNotFound(archive_path));
    }

    let local_exists = original_path.exists();

    // Check that the local filesystem has enough free space.
    // Use the nearest existing ancestor since the target may not exist yet.
    let space_check_path = if local_exists {
        original_path.clone()
    } else {
        original_path
            .ancestors()
            .find(|p| p.exists())
            .unwrap_or(Path::new("/"))
            .to_path_buf()
    };
    drive::check_space(&space_check_path, entry.size_bytes)?;

    Ok(RestorePlan {
        original_path,
        archive_path,
        drive_root: drive.root_path.clone(),
        entry,
        local_exists,
    })
}

/// Execute a restore: verify archive checksums, copy back to original location, update manifest.
pub fn execute_restore(
    plan: &RestorePlan,
    keep_archive: bool,
    progress: Option<&dyn Progress>,
) -> TuckResult<()> {
    // Step 1: Verify archive checksums before restoring
    if let Some(p) = progress {
        p.start_phase("Verifying archive", plan.entry.size_bytes);
    }
    for cs in &plan.entry.checksums {
        let file_path = if cs.relative_path.is_empty() {
            plan.archive_path.clone()
        } else {
            plan.archive_path.join(&cs.relative_path)
        };
        if !checksum::verify_checksum_with_progress(&file_path, &cs.hash, cs.size_bytes, progress)?
        {
            let actual = checksum::hash_file(&file_path)?;
            return Err(TuckError::ChecksumMismatch {
                path: file_path,
                expected: cs.hash.clone(),
                actual,
            });
        }
    }
    if let Some(p) = progress {
        p.finish_phase();
    }

    // Write pending marker before starting the copy
    let pending = PendingOperation {
        kind: PendingKind::Restore,
        original_path: plan.original_path.clone(),
        archive_path: plan.archive_path.clone(),
        started_at: Utc::now(),
    };
    PendingOperation::write(&plan.drive_root, &pending)?;

    // Step 2: Copy back to original location
    if let Some(p) = progress {
        p.start_phase("Restoring files", plan.entry.size_bytes);
    }
    copy::copy_recursive(&plan.archive_path, &plan.original_path, progress)?;
    if let Some(p) = progress {
        p.finish_phase();
    }

    // Step 3: Update manifest — remove entry
    let mut manifest = Manifest::load(&plan.drive_root)?;
    manifest.remove_entry(&plan.original_path)?;
    manifest.save(&plan.drive_root)?;

    // Step 4: Optionally remove archive copy
    if !keep_archive {
        if let Some(p) = progress {
            p.start_phase("Removing archive copy", 0);
        }
        copy::remove_path(&plan.archive_path)?;
        if let Some(p) = progress {
            p.finish_phase();
        }
    }

    // Clear pending marker — operation completed successfully
    PendingOperation::clear(&plan.drive_root)?;

    Ok(())
}

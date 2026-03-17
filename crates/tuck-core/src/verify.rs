use std::path::Path;

use crate::checksum;
use crate::drive::{archive_path_on_drive, DriveInfo};
use crate::error::{TuckError, TuckResult};
use crate::manifest::{ArchiveEntry, Manifest};
use crate::progress::Progress;

/// Result of verifying a single archive entry.
#[derive(Debug)]
pub struct VerifyResult {
    pub original_path: std::path::PathBuf,
    pub total_files: usize,
    pub passed: usize,
    pub failed: Vec<VerifyFailure>,
}

#[derive(Debug)]
pub struct VerifyFailure {
    pub relative_path: String,
    pub expected: String,
    pub actual: String,
}

impl VerifyResult {
    pub fn is_ok(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Verify a single archive entry's checksums against the files on drive.
pub fn verify_entry(
    entry: &ArchiveEntry,
    drive: &DriveInfo,
    progress: Option<&dyn Progress>,
) -> TuckResult<VerifyResult> {
    let archive_base = archive_path_on_drive(&drive.root_path, &entry.original_path);
    let mut passed = 0;
    let mut failed = Vec::new();

    for cs in &entry.checksums {
        let file_path = if cs.relative_path.is_empty() {
            archive_base.clone()
        } else {
            archive_base.join(&cs.relative_path)
        };

        if !file_path.exists() {
            failed.push(VerifyFailure {
                relative_path: cs.relative_path.clone(),
                expected: cs.hash.clone(),
                actual: "<missing>".to_string(),
            });
            continue;
        }

        let actual_hash = checksum::hash_file(&file_path)?;
        if let Some(p) = progress {
            p.advance(cs.size_bytes);
        }
        if actual_hash == cs.hash {
            passed += 1;
        } else {
            failed.push(VerifyFailure {
                relative_path: cs.relative_path.clone(),
                expected: cs.hash.clone(),
                actual: actual_hash,
            });
        }
    }

    Ok(VerifyResult {
        original_path: entry.original_path.clone(),
        total_files: entry.checksums.len(),
        passed,
        failed,
    })
}

/// Verify all entries in the manifest.
pub fn verify_all(
    drive: &DriveInfo,
    progress: Option<&dyn Progress>,
) -> TuckResult<Vec<VerifyResult>> {
    let manifest = Manifest::load(&drive.root_path)?;

    if let Some(p) = progress {
        let total: u64 = manifest.entries.iter().map(|e| e.size_bytes).sum();
        p.start_phase("Verifying checksums", total);
    }

    let mut results = Vec::new();
    for entry in &manifest.entries {
        results.push(verify_entry(entry, drive, progress)?);
    }

    if let Some(p) = progress {
        p.finish_phase();
    }

    Ok(results)
}

/// Check the status of a path: is it archived on the given drive?
pub fn check_status(path: &Path, drive: &DriveInfo) -> TuckResult<Option<ArchiveEntry>> {
    let original_path = if path.exists() {
        path.canonicalize().map_err(|_| TuckError::PathNotFound(path.to_path_buf()))?
    } else if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| TuckError::Io {
                source: e,
                path: path.to_path_buf(),
            })?
            .join(path)
    };

    let manifest = Manifest::load(&drive.root_path)?;
    Ok(manifest.find_entry(&original_path).cloned())
}

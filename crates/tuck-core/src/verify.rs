use std::fs;
use std::path::Path;

use crate::checksum;
use crate::drive::{safe_archive_path, DriveInfo};
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
    let mut passed = 0;
    let mut failed = Vec::new();
    let mut fail = |cs: &crate::manifest::FileChecksum, actual: &str| {
        failed.push(VerifyFailure {
            relative_path: cs.relative_path.clone(),
            expected: cs.hash.clone(),
            actual: actual.to_string(),
        });
    };

    // The entry and its checksum list come from the manifest on the drive.
    // Paths that would lead outside the archive (absolute, `..`, or through a
    // symlink) are reported as failures; they are never opened.
    let archive_base = safe_archive_path(&drive.root_path, &entry.original_path).ok();

    for cs in &entry.checksums {
        let file_path = match archive_base.as_deref().map(|base| cs.path_under(base)) {
            Some(Ok(path)) => path,
            _ => {
                fail(cs, "<unsafe path in manifest>");
                continue;
            }
        };

        match fs::symlink_metadata(&file_path) {
            Ok(meta) if meta.file_type().is_file() => {}
            Ok(_) => {
                fail(cs, "<not a regular file>");
                continue;
            }
            Err(_) => {
                fail(cs, "<missing>");
                continue;
            }
        }

        let actual_hash = checksum::hash_file(&file_path)?;
        if let Some(p) = progress {
            p.advance(cs.size_bytes);
        }
        if actual_hash == cs.hash {
            passed += 1;
        } else {
            fail(cs, &actual_hash);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::FileChecksum;
    use chrono::Utc;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn drive(root: &Path) -> DriveInfo {
        DriveInfo {
            name: "TestDrive".to_string(),
            mount_path: root.to_path_buf(),
            root_path: root.to_path_buf(),
        }
    }

    fn entry(original: &str, files: &[(&str, &str)]) -> ArchiveEntry {
        ArchiveEntry {
            original_path: original.into(),
            is_directory: true,
            archived_at: Utc::now(),
            size_bytes: 0,
            checksums: files
                .iter()
                .map(|(rel, hash)| FileChecksum {
                    relative_path: rel.to_string(),
                    hash: hash.to_string(),
                    size_bytes: 0,
                })
                .collect(),
            drive_name: "TestDrive".to_string(),
        }
    }

    #[test]
    fn test_verify_entry_pass_fail_and_missing() {
        let root = TempDir::new().unwrap();
        let base = root.path().join("Users/me/project");
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("good.txt"), "good").unwrap();
        fs::write(base.join("bad.txt"), "changed").unwrap();
        let good_hash = checksum::hash_file(&base.join("good.txt")).unwrap();

        let e = entry(
            "/Users/me/project",
            &[("good.txt", &good_hash), ("bad.txt", "0000"), ("gone.txt", "0000")],
        );
        let result = verify_entry(&e, &drive(root.path()), None).unwrap();

        assert_eq!(result.passed, 1);
        assert_eq!(result.failed.len(), 2);
        assert_eq!(result.failed[1].actual, "<missing>");
    }

    #[test]
    fn test_verify_never_reads_outside_the_archive() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let secret = outside.path().join("id_rsa");
        fs::write(&secret, "PRIVATE KEY").unwrap();
        let secret_hash = checksum::hash_file(&secret).unwrap();

        let base = root.path().join("Users/me/project");
        fs::create_dir_all(&base).unwrap();
        symlink(&secret, base.join("link")).unwrap();
        symlink(outside.path(), base.join("dir")).unwrap();

        // Each of these would have hashed the outside file and reported the
        // result; with the correct hash supplied, they would even have passed.
        let secret_str = secret.to_string_lossy();
        let dotdot = format!("../../../../../../..{secret_str}");
        let e = entry(
            "/Users/me/project",
            &[
                (secret_str.as_ref(), &secret_hash),
                (&dotdot, &secret_hash),
                ("link", &secret_hash),
                ("dir/id_rsa", &secret_hash),
            ],
        );
        let result = verify_entry(&e, &drive(root.path()), None).unwrap();

        assert_eq!(result.passed, 0);
        assert_eq!(result.failed.len(), 4);
        assert!(result.failed.iter().all(|f| !f.actual.contains(&secret_hash)));
    }

    #[test]
    fn test_verify_rejects_entry_path_escaping_the_drive() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let secret = outside.path().canonicalize().unwrap().join("id_rsa");
        fs::write(&secret, "PRIVATE KEY").unwrap();
        let secret_hash = checksum::hash_file(&secret).unwrap();

        let escaping = format!("/../../../../../..{}", secret.display());
        let mut e = entry(&escaping, &[("", &secret_hash)]);
        e.is_directory = false;
        let result = verify_entry(&e, &drive(root.path()), None).unwrap();

        assert_eq!(result.passed, 0);
        assert_eq!(result.failed[0].actual, "<unsafe path in manifest>");
    }
}

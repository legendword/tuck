use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::copy;
use crate::error::{IoContext, TuckError, TuckResult};

const PENDING_FILENAME: &str = ".tuck-pending.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PendingKind {
    /// An add operation was in progress: files were being copied to the drive.
    Add,
    /// A restore operation was in progress: files were being copied from the drive.
    Restore,
}

/// Tracks an in-progress operation so it can be cleaned up after interruption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOperation {
    pub kind: PendingKind,
    /// The original local path.
    pub original_path: PathBuf,
    /// The path on the drive (for add: destination being written; for restore: source).
    pub archive_path: PathBuf,
    /// When this operation started.
    pub started_at: DateTime<Utc>,
}

impl PendingOperation {
    /// Path to the pending file on the given drive root.
    pub fn path_on_drive(drive_root: &Path) -> PathBuf {
        drive_root.join(PENDING_FILENAME)
    }

    /// Write a pending marker before starting an operation.
    pub fn write(drive_root: &Path, op: &PendingOperation) -> TuckResult<()> {
        let path = Self::path_on_drive(drive_root);
        let data = serde_json::to_string_pretty(op)
            .map_err(|e| TuckError::Other(e.to_string()))?;
        fs::write(&path, &data).io_context(&path)?;
        Ok(())
    }

    /// Remove the pending marker after an operation completes successfully.
    pub fn clear(drive_root: &Path) -> TuckResult<()> {
        let path = Self::path_on_drive(drive_root);
        if path.exists() {
            fs::remove_file(&path).io_context(&path)?;
        }
        Ok(())
    }

    /// Check if there's a pending operation on this drive root.
    pub fn load(drive_root: &Path) -> TuckResult<Option<PendingOperation>> {
        let path = Self::path_on_drive(drive_root);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path).io_context(&path)?;
        let op = serde_json::from_str(&data)
            .map_err(|e| TuckError::Other(format!("Invalid pending file: {}", e)))?;
        Ok(Some(op))
    }

    /// Clean up the partial files left by an interrupted operation.
    pub fn cleanup(drive_root: &Path, op: &PendingOperation) -> TuckResult<()> {
        match op.kind {
            PendingKind::Add => {
                // Interrupted add: remove partial copy on the drive
                if op.archive_path.exists() {
                    copy::remove_path(&op.archive_path)?;
                }
            }
            PendingKind::Restore => {
                // Interrupted restore: remove partial copy at the local path.
                // The archive on the drive is still intact — the manifest entry
                // was only removed *after* copy completed, so it's still there.
                if op.original_path.exists() {
                    copy::remove_path(&op.original_path)?;
                }
            }
        }
        Self::clear(drive_root)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_write_and_load() {
        let dir = TempDir::new().unwrap();
        let op = PendingOperation {
            kind: PendingKind::Add,
            original_path: PathBuf::from("/Users/test/file.txt"),
            archive_path: PathBuf::from("/Volumes/Drive/Users/test/file.txt"),
            started_at: Utc::now(),
        };
        PendingOperation::write(dir.path(), &op).unwrap();

        let loaded = PendingOperation::load(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.original_path, op.original_path);
        assert_eq!(loaded.archive_path, op.archive_path);
    }

    #[test]
    fn test_load_none() {
        let dir = TempDir::new().unwrap();
        assert!(PendingOperation::load(dir.path()).unwrap().is_none());
    }

    #[test]
    fn test_clear() {
        let dir = TempDir::new().unwrap();
        let op = PendingOperation {
            kind: PendingKind::Add,
            original_path: PathBuf::from("/test"),
            archive_path: PathBuf::from("/drive/test"),
            started_at: Utc::now(),
        };
        PendingOperation::write(dir.path(), &op).unwrap();
        assert!(PendingOperation::path_on_drive(dir.path()).exists());

        PendingOperation::clear(dir.path()).unwrap();
        assert!(!PendingOperation::path_on_drive(dir.path()).exists());
    }

    #[test]
    fn test_cleanup_add_removes_archive_path() {
        let dir = TempDir::new().unwrap();
        let partial = dir.path().join("partial_file.txt");
        fs::write(&partial, "partial data").unwrap();

        let op = PendingOperation {
            kind: PendingKind::Add,
            original_path: PathBuf::from("/Users/test/file.txt"),
            archive_path: partial.clone(),
            started_at: Utc::now(),
        };
        PendingOperation::write(dir.path(), &op).unwrap();

        PendingOperation::cleanup(dir.path(), &op).unwrap();
        assert!(!partial.exists());
        assert!(!PendingOperation::path_on_drive(dir.path()).exists());
    }

    #[test]
    fn test_cleanup_restore_removes_local_path() {
        let dir = TempDir::new().unwrap();
        let partial_local = dir.path().join("partial_restore.txt");
        fs::write(&partial_local, "partial data").unwrap();

        let op = PendingOperation {
            kind: PendingKind::Restore,
            original_path: partial_local.clone(),
            archive_path: PathBuf::from("/Volumes/Drive/test"),
            started_at: Utc::now(),
        };
        PendingOperation::write(dir.path(), &op).unwrap();

        PendingOperation::cleanup(dir.path(), &op).unwrap();
        assert!(!partial_local.exists());
        assert!(!PendingOperation::path_on_drive(dir.path()).exists());
    }
}

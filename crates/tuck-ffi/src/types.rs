use std::path::PathBuf;

use tuck_core::archive::AddPlan;
use tuck_core::drive::DriveInfo;
use tuck_core::manifest::{ArchiveEntry, FileChecksum};
use tuck_core::pending::{CleanupPlan, PendingKind, PendingStatus};
use tuck_core::restore::RestorePlan;
use tuck_core::safety::printable;

// --- DriveInfo ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiDriveInfo {
    pub name: String,
    pub mount_path: String,
    pub root_path: String,
}

impl From<DriveInfo> for FfiDriveInfo {
    fn from(d: DriveInfo) -> Self {
        FfiDriveInfo {
            name: d.name,
            mount_path: d.mount_path.to_string_lossy().into_owned(),
            root_path: d.root_path.to_string_lossy().into_owned(),
        }
    }
}

impl From<FfiDriveInfo> for DriveInfo {
    fn from(d: FfiDriveInfo) -> Self {
        DriveInfo {
            name: d.name,
            mount_path: PathBuf::from(d.mount_path),
            root_path: PathBuf::from(d.root_path),
        }
    }
}

// --- FileChecksum ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiFileChecksum {
    pub relative_path: String,
    pub hash: String,
    pub size_bytes: u64,
}

impl From<FileChecksum> for FfiFileChecksum {
    fn from(c: FileChecksum) -> Self {
        FfiFileChecksum {
            relative_path: c.relative_path,
            hash: c.hash,
            size_bytes: c.size_bytes,
        }
    }
}

// --- ArchiveEntry ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiArchiveEntry {
    pub original_path: String,
    pub is_directory: bool,
    pub archived_at: i64,
    pub size_bytes: u64,
    pub checksums: Vec<FfiFileChecksum>,
    pub drive_name: String,
}

impl From<ArchiveEntry> for FfiArchiveEntry {
    fn from(e: ArchiveEntry) -> Self {
        FfiArchiveEntry {
            original_path: e.original_path.to_string_lossy().into_owned(),
            is_directory: e.is_directory,
            archived_at: e.archived_at.timestamp(),
            size_bytes: e.size_bytes,
            checksums: e.checksums.into_iter().map(Into::into).collect(),
            drive_name: e.drive_name,
        }
    }
}

// --- AddPlan ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAddPlan {
    pub original_path: String,
    pub archive_path: String,
    pub drive_name: String,
    pub drive_root: String,
    pub is_directory: bool,
    pub size_bytes: u64,
}

impl From<AddPlan> for FfiAddPlan {
    fn from(p: AddPlan) -> Self {
        FfiAddPlan {
            original_path: p.original_path.to_string_lossy().into_owned(),
            archive_path: p.archive_path.to_string_lossy().into_owned(),
            drive_name: p.drive_name,
            drive_root: p.drive_root.to_string_lossy().into_owned(),
            is_directory: p.is_directory,
            size_bytes: p.size_bytes,
        }
    }
}

impl From<FfiAddPlan> for AddPlan {
    fn from(p: FfiAddPlan) -> Self {
        AddPlan {
            original_path: PathBuf::from(p.original_path),
            archive_path: PathBuf::from(p.archive_path),
            drive_name: p.drive_name,
            drive_root: PathBuf::from(p.drive_root),
            is_directory: p.is_directory,
            size_bytes: p.size_bytes,
        }
    }
}

// --- PendingOperation ---

#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiPendingKind {
    Add,
    Restore,
    /// The marker on the drive could not be read.
    Unknown,
}

/// An interrupted-operation marker found on the drive, already vetted by
/// tuck-core. The marker itself is untrusted (anyone can write one to a
/// drive), so the UI should present `message` and `delete_path` rather than
/// interpreting the raw paths.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiPendingOperation {
    pub kind: FfiPendingKind,
    pub original_path: String,
    pub archive_path: String,
    pub started_at: i64,
    /// The one path that cleaning up will delete, or `None` when cleanup only
    /// removes the marker.
    pub delete_path: Option<String>,
    /// What cleaning up will do, ready to show to the user.
    pub message: String,
}

impl From<PendingStatus> for FfiPendingOperation {
    fn from(status: PendingStatus) -> Self {
        let kind = match status.op.as_ref().map(|op| op.kind) {
            Some(PendingKind::Add) => FfiPendingKind::Add,
            Some(PendingKind::Restore) => FfiPendingKind::Restore,
            None => FfiPendingKind::Unknown,
        };
        let (delete_path, message) = match &status.plan {
            CleanupPlan::Remove(target) => {
                let shown = printable(target.display());
                let message = match kind {
                    FfiPendingKind::Restore => format!(
                        "A restore started on this Mac was interrupted. Cleaning up will \
                         permanently delete the partial local copy:\n\n{shown}\n\n\
                         The archive on the drive is not affected."
                    ),
                    _ => format!(
                        "A previous add was interrupted. Cleaning up will permanently \
                         delete the partial copy on the drive:\n\n{shown}\n\n\
                         Your local files are not affected."
                    ),
                };
                (Some(target.to_string_lossy().into_owned()), message)
            }
            CleanupPlan::DiscardMarker { reason } => (
                None,
                format!(
                    "The drive has a marker left by an interrupted operation. Nothing \
                     will be deleted: {}. Cleaning up only removes the marker.",
                    printable(reason)
                ),
            ),
        };
        let op = status.op;
        FfiPendingOperation {
            kind,
            original_path: op
                .as_ref()
                .map(|op| op.original_path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            archive_path: op
                .as_ref()
                .map(|op| op.archive_path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            started_at: op.as_ref().map(|op| op.started_at.timestamp()).unwrap_or(0),
            delete_path,
            message,
        }
    }
}

// --- RestorePlan ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiRestorePlan {
    pub original_path: String,
    pub archive_path: String,
    pub drive_root: String,
    pub entry: FfiArchiveEntry,
    pub local_exists: bool,
}

impl From<RestorePlan> for FfiRestorePlan {
    fn from(p: RestorePlan) -> Self {
        FfiRestorePlan {
            original_path: p.original_path.to_string_lossy().into_owned(),
            archive_path: p.archive_path.to_string_lossy().into_owned(),
            drive_root: p.drive_root.to_string_lossy().into_owned(),
            entry: p.entry.into(),
            local_exists: p.local_exists,
        }
    }
}

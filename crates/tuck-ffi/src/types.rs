use std::path::PathBuf;

use tuck_core::archive::AddPlan;
use tuck_core::drive::DriveInfo;
use tuck_core::manifest::{ArchiveEntry, FileChecksum};
use tuck_core::restore::RestorePlan;

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
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiPendingOperation {
    pub kind: FfiPendingKind,
    pub original_path: String,
    pub archive_path: String,
    pub started_at: i64,
}

impl From<tuck_core::pending::PendingOperation> for FfiPendingOperation {
    fn from(op: tuck_core::pending::PendingOperation) -> Self {
        FfiPendingOperation {
            kind: match op.kind {
                tuck_core::pending::PendingKind::Add => FfiPendingKind::Add,
                tuck_core::pending::PendingKind::Restore => FfiPendingKind::Restore,
            },
            original_path: op.original_path.to_string_lossy().into_owned(),
            archive_path: op.archive_path.to_string_lossy().into_owned(),
            started_at: op.started_at.timestamp(),
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

use std::path::Path;

use tuck_core::archive;
use tuck_core::drive;
use tuck_core::manifest::Manifest;
use tuck_core::pending::PendingOperation;
use tuck_core::progress::Progress;
use tuck_core::restore;

use crate::error::FfiTuckError;
use crate::progress::{FfiProgress, ProgressBridge};
use crate::types::{
    FfiAddPlan, FfiArchiveEntry, FfiDriveInfo, FfiFileChecksum, FfiPendingOperation, FfiRestorePlan,
};

#[uniffi::export]
pub fn list_drives() -> Result<Vec<FfiDriveInfo>, FfiTuckError> {
    drive::list_drives()
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(Into::into)
}

#[uniffi::export]
pub fn resolve_drive(
    name: Option<String>,
    prefix: Option<String>,
) -> Result<FfiDriveInfo, FfiTuckError> {
    drive::resolve_drive(name.as_deref(), prefix.as_deref())
        .map(Into::into)
        .map_err(Into::into)
}

#[uniffi::export]
pub fn plan_add(path: String, drive: FfiDriveInfo) -> Result<FfiAddPlan, FfiTuckError> {
    let drive_info: tuck_core::drive::DriveInfo = drive.into();
    archive::plan_add(Path::new(&path), &drive_info)
        .map(Into::into)
        .map_err(Into::into)
}

#[uniffi::export]
pub fn execute_add(
    plan: FfiAddPlan,
    progress: Option<Box<dyn FfiProgress>>,
) -> Result<Vec<FfiFileChecksum>, FfiTuckError> {
    let core_plan: tuck_core::archive::AddPlan = plan.into();
    let bridge = progress.map(|p| ProgressBridge { inner: p });
    let progress_ref: Option<&dyn Progress> = bridge.as_ref().map(|b| b as &dyn Progress);
    archive::execute_add(&core_plan, progress_ref)
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(Into::into)
}

#[uniffi::export]
pub fn delete_local(path: String) -> Result<(), FfiTuckError> {
    archive::delete_local(Path::new(&path)).map_err(Into::into)
}

#[uniffi::export]
pub fn plan_restore(path: String, drive: FfiDriveInfo) -> Result<FfiRestorePlan, FfiTuckError> {
    let drive_info: tuck_core::drive::DriveInfo = drive.into();
    restore::plan_restore(Path::new(&path), &drive_info)
        .map(Into::into)
        .map_err(Into::into)
}

#[uniffi::export]
pub fn execute_restore(
    plan: FfiRestorePlan,
    keep_archive: bool,
    progress: Option<Box<dyn FfiProgress>>,
) -> Result<(), FfiTuckError> {
    let entry: tuck_core::manifest::ArchiveEntry = archive_entry_from_ffi(&plan.entry);
    let core_plan = tuck_core::restore::RestorePlan {
        original_path: plan.original_path.into(),
        archive_path: plan.archive_path.into(),
        drive_root: plan.drive_root.into(),
        entry,
        local_exists: plan.local_exists,
    };
    let bridge = progress.map(|p| ProgressBridge { inner: p });
    let progress_ref: Option<&dyn Progress> = bridge.as_ref().map(|b| b as &dyn Progress);
    restore::execute_restore(&core_plan, keep_archive, progress_ref).map_err(Into::into)
}

#[uniffi::export]
pub fn load_manifest_entries(drive_root: String) -> Result<Vec<FfiArchiveEntry>, FfiTuckError> {
    let manifest = Manifest::load(Path::new(&drive_root)).map_err(FfiTuckError::from)?;
    Ok(manifest.entries.into_iter().map(Into::into).collect())
}

#[uniffi::export]
pub fn load_pending(drive_root: String) -> Result<Option<FfiPendingOperation>, FfiTuckError> {
    PendingOperation::load(Path::new(&drive_root))
        .map(|opt| opt.map(Into::into))
        .map_err(Into::into)
}

#[uniffi::export]
pub fn cleanup_pending(drive_root: String) -> Result<(), FfiTuckError> {
    let op = PendingOperation::load(Path::new(&drive_root)).map_err(FfiTuckError::from)?;
    if let Some(op) = op {
        PendingOperation::cleanup(Path::new(&drive_root), &op).map_err(FfiTuckError::from)?;
    }
    Ok(())
}

/// Convert an FfiArchiveEntry back to a core ArchiveEntry.
fn archive_entry_from_ffi(e: &FfiArchiveEntry) -> tuck_core::manifest::ArchiveEntry {
    use chrono::{DateTime, Utc};
    tuck_core::manifest::ArchiveEntry {
        original_path: e.original_path.clone().into(),
        is_directory: e.is_directory,
        archived_at: DateTime::<Utc>::from_timestamp(e.archived_at, 0)
            .unwrap_or_else(|| Utc::now()),
        size_bytes: e.size_bytes,
        checksums: e
            .checksums
            .iter()
            .map(|c| tuck_core::manifest::FileChecksum {
                relative_path: c.relative_path.clone(),
                hash: c.hash.clone(),
                size_bytes: c.size_bytes,
            })
            .collect(),
        drive_name: e.drive_name.clone(),
    }
}

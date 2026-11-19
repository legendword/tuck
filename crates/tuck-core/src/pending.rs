use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::copy;
use crate::drive::safe_archive_path;
use crate::error::{IoContext, TuckError, TuckResult};
use crate::manifest::Manifest;
use crate::safety;

const PENDING_FILENAME: &str = ".tuck-pending.json";
const JOURNAL_FILENAME: &str = "restore-journal.json";
/// A real marker is a few hundred bytes; anything far larger is not ours.
const PENDING_MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingKind {
    /// An add operation was in progress: files were being copied to the drive.
    Add,
    /// A restore operation was in progress: files were being copied from the drive.
    Restore,
}

/// Tracks an in-progress operation so it can be cleaned up after interruption.
///
/// The marker is stored on the drive, so when it is read back it is untrusted
/// input: anyone who can write to the drive can author one. It is never acted
/// on directly. [`PendingOperation::inspect`] works out what, if anything, is
/// safe to delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingOperation {
    pub kind: PendingKind,
    /// The original local path.
    pub original_path: PathBuf,
    /// The path on the drive (for add: destination being written; for restore: source).
    /// Informational only: the location is always recomputed from
    /// `original_path` and the drive root before anything is deleted.
    pub archive_path: PathBuf,
    /// When this operation started.
    pub started_at: DateTime<Utc>,
}

/// What cleaning up a pending marker will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupPlan {
    /// Delete this path (the partial copy), then remove the marker.
    Remove(PathBuf),
    /// Delete nothing; only remove the marker. `reason` says why.
    DiscardMarker { reason: String },
}

/// A pending marker found on a drive, together with the vetted cleanup plan.
#[derive(Debug, Clone)]
pub struct PendingStatus {
    /// The parsed marker, or `None` if the marker file could not be parsed.
    pub op: Option<PendingOperation>,
    pub plan: CleanupPlan,
}

/// Local record of the restores this machine has in progress.
///
/// Cleaning up an interrupted restore means deleting a *local* path, and the
/// marker on the drive cannot authorize that by itself. The journal lives in
/// the user's own config directory, so a matching record proves that tuck on
/// this machine really started that restore to that path.
#[derive(Debug, Clone)]
pub struct RestoreJournal {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalRecord {
    drive_root: PathBuf,
    original_path: PathBuf,
    started_at: DateTime<Utc>,
}

impl RestoreJournal {
    /// The journal next to the config file: `~/.config/tuck/restore-journal.json`.
    pub fn default_location() -> Self {
        Self::at(Config::path().with_file_name(JOURNAL_FILENAME))
    }

    /// A journal stored at an explicit path (used by tests).
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn load(&self) -> Vec<JournalRecord> {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    fn save(&self, records: &[JournalRecord]) -> TuckResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).io_context(parent)?;
        }
        let data = serde_json::to_string_pretty(records)
            .map_err(|e| TuckError::Other(e.to_string()))?;
        safety::write_metadata_file(&self.path, data.as_bytes())
    }

    /// Record that a restore is starting. Replaces any earlier record for the
    /// same drive root: a drive root has at most one operation in flight.
    pub fn record(&self, drive_root: &Path, op: &PendingOperation) -> TuckResult<()> {
        let mut records = self.load();
        records.retain(|r| r.drive_root != drive_root);
        records.push(JournalRecord {
            drive_root: drive_root.to_path_buf(),
            original_path: op.original_path.clone(),
            started_at: op.started_at,
        });
        self.save(&records)
    }

    /// Drop the record for a drive root (the operation finished, was cleaned
    /// up, or has been superseded by a new one).
    pub fn forget_drive(&self, drive_root: &Path) -> TuckResult<()> {
        let mut records = self.load();
        let before = records.len();
        records.retain(|r| r.drive_root != drive_root);
        if records.len() != before {
            self.save(&records)?;
        }
        Ok(())
    }

    /// True if this machine started exactly this restore on this drive root.
    pub fn contains(&self, drive_root: &Path, op: &PendingOperation) -> bool {
        self.load().iter().any(|r| {
            r.drive_root == drive_root
                && r.original_path == op.original_path
                && r.started_at == op.started_at
        })
    }
}

/// Result of reading the marker file.
enum Marker {
    Missing,
    Valid(PendingOperation),
    /// The file exists but is not a usable marker; the string says why.
    Invalid(String),
}

fn read_marker(drive_root: &Path) -> TuckResult<Marker> {
    let path = PendingOperation::path_on_drive(drive_root);
    let data = match safety::read_metadata_file(&path, Some(PENDING_MAX_BYTES)) {
        Ok(Some(data)) => data,
        Ok(None) => return Ok(Marker::Missing),
        Err(TuckError::UnsafePath { reason, .. }) => return Ok(Marker::Invalid(reason)),
        Err(e) => return Err(e),
    };
    Ok(match serde_json::from_str(&data) {
        Ok(op) => Marker::Valid(op),
        Err(e) => Marker::Invalid(format!("it is not a valid tuck marker: {e}")),
    })
}

fn discard(reason: impl Into<String>) -> CleanupPlan {
    CleanupPlan::DiscardMarker {
        reason: reason.into(),
    }
}

/// Decide what cleaning up `op` may delete. Every check here exists because
/// `op` was read from the drive and may have been written by someone else.
fn classify(
    drive_root: &Path,
    op: &PendingOperation,
    journal: &RestoreJournal,
) -> TuckResult<CleanupPlan> {
    // Recompute the drive-side location instead of trusting `op.archive_path`.
    // This also rejects `..` and symlinks that would lead off the drive.
    let archive_path = safe_archive_path(drive_root, &op.original_path);
    let listed = Manifest::load(drive_root)?
        .find_entry(&op.original_path)
        .is_some();

    Ok(match op.kind {
        PendingKind::Add => match archive_path {
            Err(e) => discard(format!(
                "the marker does not point safely inside this drive ({e})"
            )),
            // The entry is only written once the copy has been verified, so
            // what is on the drive is a complete archive, not a partial copy.
            Ok(_) if listed => {
                discard("the manifest already lists this path, so the archive is complete")
            }
            Ok(path) if fs::symlink_metadata(&path).is_err() => {
                discard("no partial copy was found on the drive")
            }
            Ok(path) => CleanupPlan::Remove(path),
        },
        PendingKind::Restore => {
            if !journal.contains(drive_root, op) {
                discard(
                    "this restore was not started by tuck on this Mac, \
                     so no local files will be deleted for it",
                )
            } else if !listed {
                // The entry is removed from the manifest only after the copy
                // back has completed.
                discard("the restore had finished copying, so the local copy is complete")
            } else if !archive_path.is_ok_and(|p| fs::symlink_metadata(p).is_ok()) {
                discard("the archive copy is not on the drive, so the local copy is kept")
            } else if fs::symlink_metadata(&op.original_path).is_err() {
                discard("no partial local copy was found")
            } else {
                CleanupPlan::Remove(op.original_path.clone())
            }
        }
    })
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
        safety::write_metadata_file(&path, data.as_bytes())
    }

    /// Remove the pending marker after an operation completes successfully.
    pub fn clear(drive_root: &Path) -> TuckResult<()> {
        safety::remove_file_if_present(&Self::path_on_drive(drive_root))
    }

    /// Read the pending marker on this drive root, if there is one.
    ///
    /// This only parses the marker. Use [`PendingOperation::inspect`] to find
    /// out what cleaning it up would do.
    pub fn load(drive_root: &Path) -> TuckResult<Option<PendingOperation>> {
        match read_marker(drive_root)? {
            Marker::Missing => Ok(None),
            Marker::Valid(op) => Ok(Some(op)),
            Marker::Invalid(reason) => {
                Err(TuckError::Other(format!("Invalid pending file: {reason}")))
            }
        }
    }

    /// Check for a pending marker and work out a safe cleanup plan for it.
    ///
    /// Returns `Ok(None)` when there is no marker. A marker that cannot be
    /// parsed or trusted is still reported, with a plan that only discards it,
    /// so a bad marker never blocks the drive and never deletes anything.
    pub fn inspect(drive_root: &Path) -> TuckResult<Option<PendingStatus>> {
        Self::inspect_with_journal(drive_root, &RestoreJournal::default_location())
    }

    /// [`PendingOperation::inspect`] with an explicit journal location.
    pub fn inspect_with_journal(
        drive_root: &Path,
        journal: &RestoreJournal,
    ) -> TuckResult<Option<PendingStatus>> {
        Ok(match read_marker(drive_root)? {
            Marker::Missing => None,
            Marker::Invalid(reason) => Some(PendingStatus {
                op: None,
                plan: discard(format!("the marker file is not usable ({reason})")),
            }),
            Marker::Valid(op) => Some(PendingStatus {
                plan: classify(drive_root, &op, journal)?,
                op: Some(op),
            }),
        })
    }
}

impl PendingStatus {
    /// Carry out the cleanup plan, then remove the marker.
    pub fn resolve(&self, drive_root: &Path) -> TuckResult<()> {
        self.resolve_with_journal(drive_root, &RestoreJournal::default_location())
    }

    /// [`PendingStatus::resolve`] with an explicit journal location.
    pub fn resolve_with_journal(
        &self,
        drive_root: &Path,
        journal: &RestoreJournal,
    ) -> TuckResult<()> {
        // Re-derive the plan right before acting. This keeps the checks next
        // to the deletion and means a hand-built status cannot be used to
        // delete an arbitrary path.
        let current = match &self.op {
            Some(op) => classify(drive_root, op, journal)?,
            None => discard(String::new()),
        };
        let unchanged = match (&self.plan, &current) {
            (CleanupPlan::Remove(a), CleanupPlan::Remove(b)) => a == b,
            (CleanupPlan::DiscardMarker { .. }, CleanupPlan::DiscardMarker { .. }) => true,
            _ => false,
        };
        if !unchanged {
            return Err(TuckError::Other(
                "The interrupted operation changed while it was being reviewed. \
                 Run the command again."
                    .to_string(),
            ));
        }

        if let CleanupPlan::Remove(target) = &current {
            copy::remove_path(target)?;
        }
        // Whatever was pending on this drive root is now settled.
        let _ = journal.forget_drive(drive_root);
        PendingOperation::clear(drive_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::archive_path_on_drive;
    use crate::manifest::ArchiveEntry;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    struct Fixture {
        drive: TempDir,
        local: TempDir,
        journal: RestoreJournal,
    }

    impl Fixture {
        fn new() -> Self {
            let local = TempDir::new().unwrap();
            let journal = RestoreJournal::at(local.path().join("state/restore-journal.json"));
            Fixture {
                drive: TempDir::new().unwrap(),
                local,
                journal,
            }
        }

        fn root(&self) -> &Path {
            self.drive.path()
        }

        /// A local directory with one file in it; returns its canonical path.
        fn local_dir(&self, name: &str) -> PathBuf {
            let dir = self.local.path().canonicalize().unwrap().join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("file.txt"), "precious").unwrap();
            dir
        }

        fn op(&self, kind: PendingKind, original: &Path) -> PendingOperation {
            PendingOperation {
                kind,
                original_path: original.to_path_buf(),
                archive_path: archive_path_on_drive(self.root(), original),
                started_at: Utc::now(),
            }
        }

        fn add_manifest_entry(&self, original: &Path) {
            let mut manifest = Manifest::load(self.root()).unwrap();
            manifest
                .add_entry(ArchiveEntry {
                    original_path: original.to_path_buf(),
                    is_directory: true,
                    archived_at: Utc::now(),
                    size_bytes: 0,
                    checksums: Vec::new(),
                    drive_name: "TestDrive".to_string(),
                })
                .unwrap();
            manifest.save(self.root()).unwrap();
        }

        fn inspect(&self) -> Option<PendingStatus> {
            PendingOperation::inspect_with_journal(self.root(), &self.journal).unwrap()
        }

        fn resolve(&self, status: &PendingStatus) {
            status
                .resolve_with_journal(self.root(), &self.journal)
                .unwrap();
        }

        fn marker_exists(&self) -> bool {
            fs::symlink_metadata(PendingOperation::path_on_drive(self.root())).is_ok()
        }
    }

    fn is_discard(status: &PendingStatus) -> bool {
        matches!(status.plan, CleanupPlan::DiscardMarker { .. })
    }

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
        assert_eq!(loaded, op);
    }

    #[test]
    fn test_load_none() {
        let f = Fixture::new();
        assert!(PendingOperation::load(f.root()).unwrap().is_none());
        assert!(f.inspect().is_none());
    }

    #[test]
    fn test_clear() {
        let f = Fixture::new();
        let op = f.op(PendingKind::Add, Path::new("/test"));
        PendingOperation::write(f.root(), &op).unwrap();
        assert!(f.marker_exists());

        PendingOperation::clear(f.root()).unwrap();
        assert!(!f.marker_exists());
        // Clearing again is not an error.
        PendingOperation::clear(f.root()).unwrap();
    }

    #[test]
    fn test_write_does_not_follow_planted_symlink() {
        let f = Fixture::new();
        let victim = f.local.path().join("authorized_keys");
        // Dangling symlink: the old code created the target through it.
        symlink(&victim, PendingOperation::path_on_drive(f.root())).unwrap();

        let op = f.op(PendingKind::Add, Path::new("/Users/test/file.txt"));
        PendingOperation::write(f.root(), &op).unwrap();

        assert!(fs::symlink_metadata(&victim).is_err());
        assert_eq!(PendingOperation::load(f.root()).unwrap().unwrap(), op);
    }

    #[test]
    fn test_interrupted_add_removes_partial_copy_on_drive() {
        let f = Fixture::new();
        let original = Path::new("/Users/test/project");
        let op = f.op(PendingKind::Add, original);
        fs::create_dir_all(&op.archive_path).unwrap();
        fs::write(op.archive_path.join("partial.bin"), "partial").unwrap();
        PendingOperation::write(f.root(), &op).unwrap();

        let status = f.inspect().unwrap();
        assert_eq!(status.plan, CleanupPlan::Remove(op.archive_path.clone()));
        f.resolve(&status);

        assert!(!op.archive_path.exists());
        assert!(!f.marker_exists());
    }

    #[test]
    fn test_add_marker_cannot_delete_outside_the_drive() {
        let f = Fixture::new();
        let victim = f.local_dir("Documents");

        // archive_path names a local directory; original_path looks harmless.
        let mut op = f.op(PendingKind::Add, Path::new("/Users/me/Movies/holiday.mov"));
        op.archive_path = victim.clone();
        PendingOperation::write(f.root(), &op).unwrap();

        let status = f.inspect().unwrap();
        assert!(is_discard(&status));
        f.resolve(&status);

        assert!(victim.join("file.txt").exists());
        assert!(!f.marker_exists());
    }

    #[test]
    fn test_add_marker_with_dotdot_cannot_escape_the_drive() {
        let f = Fixture::new();
        let victim = f.local_dir("Documents");

        // /../../<victim> would resolve from the drive root back to the victim.
        let escaping = PathBuf::from(format!("/../../../../../..{}", victim.display()));
        let op = f.op(PendingKind::Add, &escaping);
        PendingOperation::write(f.root(), &op).unwrap();

        let status = f.inspect().unwrap();
        assert!(is_discard(&status));
        f.resolve(&status);
        assert!(victim.join("file.txt").exists());
    }

    #[test]
    fn test_add_marker_cannot_delete_through_symlink_on_drive() {
        let f = Fixture::new();
        let victim = f.local_dir("Documents");
        // <drive>/<first component> -> /<first component>
        let first = victim.components().nth(1).unwrap().as_os_str().to_owned();
        symlink(Path::new("/").join(&first), f.root().join(&first)).unwrap();

        let op = f.op(PendingKind::Add, &victim);
        PendingOperation::write(f.root(), &op).unwrap();

        let status = f.inspect().unwrap();
        assert!(is_discard(&status));
        f.resolve(&status);
        assert!(victim.join("file.txt").exists());
    }

    #[test]
    fn test_add_marker_does_not_delete_completed_archive() {
        let f = Fixture::new();
        let original = Path::new("/Users/test/project");
        let op = f.op(PendingKind::Add, original);
        fs::create_dir_all(&op.archive_path).unwrap();
        fs::write(op.archive_path.join("data.bin"), "only copy").unwrap();
        f.add_manifest_entry(original);
        PendingOperation::write(f.root(), &op).unwrap();

        let status = f.inspect().unwrap();
        assert!(is_discard(&status));
        f.resolve(&status);

        assert!(op.archive_path.join("data.bin").exists());
        assert!(!f.marker_exists());
    }

    #[test]
    fn test_restore_marker_without_journal_record_deletes_nothing() {
        let f = Fixture::new();
        let victim = f.local_dir("Documents");

        // Forged marker plus a consistent manifest entry and archive copy:
        // everything on the drive agrees, but this machine never started it.
        let op = f.op(PendingKind::Restore, &victim);
        fs::create_dir_all(&op.archive_path).unwrap();
        f.add_manifest_entry(&victim);
        PendingOperation::write(f.root(), &op).unwrap();

        let status = f.inspect().unwrap();
        assert!(is_discard(&status));
        f.resolve(&status);

        assert!(victim.join("file.txt").exists());
        assert!(!f.marker_exists());
    }

    #[test]
    fn test_interrupted_restore_removes_partial_local_copy() {
        let f = Fixture::new();
        let partial = f.local_dir("restoring");

        let op = f.op(PendingKind::Restore, &partial);
        fs::create_dir_all(&op.archive_path).unwrap();
        f.add_manifest_entry(&partial);
        f.journal.record(f.root(), &op).unwrap();
        PendingOperation::write(f.root(), &op).unwrap();

        let status = f.inspect().unwrap();
        assert_eq!(status.plan, CleanupPlan::Remove(partial.clone()));
        f.resolve(&status);

        assert!(!partial.exists());
        assert!(!f.marker_exists());
        assert!(!f.journal.contains(f.root(), &op));
    }

    #[test]
    fn test_restore_marker_must_match_journal_exactly() {
        let f = Fixture::new();
        let partial = f.local_dir("restoring");
        let other = f.local_dir("Documents");

        let genuine = f.op(PendingKind::Restore, &partial);
        f.journal.record(f.root(), &genuine).unwrap();

        // Same drive, different local path.
        let forged = f.op(PendingKind::Restore, &other);
        fs::create_dir_all(&forged.archive_path).unwrap();
        f.add_manifest_entry(&other);
        PendingOperation::write(f.root(), &forged).unwrap();
        assert!(is_discard(&f.inspect().unwrap()));

        // Right path, but the journal record is for another drive root.
        let other_drive = TempDir::new().unwrap();
        assert!(!f.journal.contains(other_drive.path(), &genuine));
    }

    #[test]
    fn test_restore_cleanup_keeps_completed_local_copy() {
        let f = Fixture::new();
        let restored = f.local_dir("restored");

        // Interrupted after the manifest entry was removed: the copy back had
        // finished, so the local data is the only complete copy.
        let op = f.op(PendingKind::Restore, &restored);
        f.journal.record(f.root(), &op).unwrap();
        PendingOperation::write(f.root(), &op).unwrap();

        let status = f.inspect().unwrap();
        assert!(is_discard(&status));
        f.resolve(&status);

        assert!(restored.join("file.txt").exists());
        assert!(!f.marker_exists());
    }

    #[test]
    fn test_unreadable_marker_is_discardable_not_fatal() {
        let f = Fixture::new();
        let marker = PendingOperation::path_on_drive(f.root());

        fs::write(&marker, "{ not json").unwrap();
        assert!(PendingOperation::load(f.root()).is_err());
        let status = f.inspect().unwrap();
        assert!(status.op.is_none() && is_discard(&status));
        f.resolve(&status);
        assert!(!f.marker_exists());

        // A symlink under the marker's name is reported, not followed.
        let secret = f.local.path().join("secret.json");
        fs::write(&secret, "{}").unwrap();
        symlink(&secret, &marker).unwrap();
        let status = f.inspect().unwrap();
        assert!(status.op.is_none() && is_discard(&status));
        f.resolve(&status);
        assert!(!f.marker_exists());
        assert!(secret.exists());
    }

    #[test]
    fn test_resolve_rejects_hand_built_status() {
        let f = Fixture::new();
        let victim = f.local_dir("Documents");

        for op in [
            None,
            Some(f.op(PendingKind::Add, Path::new("/Users/test/x"))),
            Some(f.op(PendingKind::Restore, &victim)),
        ] {
            let forged = PendingStatus {
                op,
                plan: CleanupPlan::Remove(victim.clone()),
            };
            assert!(forged.resolve_with_journal(f.root(), &f.journal).is_err());
            assert!(victim.join("file.txt").exists());
        }
    }

    #[test]
    fn test_journal_record_and_forget() {
        let f = Fixture::new();
        let op = f.op(PendingKind::Restore, Path::new("/Users/test/a"));
        assert!(!f.journal.contains(f.root(), &op));

        f.journal.record(f.root(), &op).unwrap();
        assert!(f.journal.contains(f.root(), &op));

        // A newer operation on the same drive root supersedes the record.
        let mut newer = f.op(PendingKind::Restore, Path::new("/Users/test/b"));
        newer.started_at = op.started_at + chrono::Duration::seconds(1);
        f.journal.record(f.root(), &newer).unwrap();
        assert!(!f.journal.contains(f.root(), &op));
        assert!(f.journal.contains(f.root(), &newer));

        f.journal.forget_drive(f.root()).unwrap();
        assert!(!f.journal.contains(f.root(), &newer));
    }
}

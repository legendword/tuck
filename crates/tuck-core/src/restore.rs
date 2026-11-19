use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use chrono::Utc;
use walkdir::WalkDir;

use crate::checksum;
use crate::copy;
use crate::drive::{self, safe_archive_path, DriveInfo};
use crate::error::{IoContext, TuckError, TuckResult};
use crate::manifest::{ArchiveEntry, Manifest};
use crate::pending::{PendingKind, PendingOperation, RestoreJournal};
use crate::progress::Progress;
use crate::safety;

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

    // The entry comes from the manifest on the drive, so its path is checked
    // rather than trusted: it must map to a location inside the tuck root.
    if drive.root_path != drive.mount_path {
        safety::ensure_contained(&drive.mount_path, &drive.root_path)?;
    }
    let archive_path = safe_archive_path(&drive.root_path, &original_path)?;

    if fs::symlink_metadata(&archive_path).is_err() {
        return Err(TuckError::PathNotFound(archive_path));
    }

    let local_exists = fs::symlink_metadata(&original_path).is_ok();

    // Check that the local filesystem has enough free space.
    // Use the nearest existing ancestor since the target may not exist yet.
    let space_check_path = if original_path.exists() {
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
///
/// If something already exists at the original location the restore is
/// refused unless `overwrite` is set. This is checked here, at the moment of
/// the copy, so every frontend gets it.
pub fn execute_restore(
    plan: &RestorePlan,
    keep_archive: bool,
    overwrite: bool,
    progress: Option<&dyn Progress>,
) -> TuckResult<()> {
    execute_restore_with_journal(
        plan,
        keep_archive,
        overwrite,
        progress,
        &RestoreJournal::default_location(),
    )
}

/// [`execute_restore`] with an explicit restore-journal location.
pub fn execute_restore_with_journal(
    plan: &RestorePlan,
    keep_archive: bool,
    overwrite: bool,
    progress: Option<&dyn Progress>,
    journal: &RestoreJournal,
) -> TuckResult<()> {
    // Re-check the plan right before acting on it: the archive location must
    // be the one for this path and must not lead off the drive.
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
    if !overwrite && fs::symlink_metadata(&plan.original_path).is_ok() {
        return Err(TuckError::AlreadyExists(plan.original_path.clone()));
    }

    // Step 1: Verify the archive before restoring
    if let Some(p) = progress {
        p.start_phase("Verifying archive", plan.entry.size_bytes);
    }
    let files = checked_archive_files(&plan.archive_path, &plan.entry)?;
    for (cs, file_path) in plan.entry.checksums.iter().zip(files) {
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

    // Record the restore locally, then write the pending marker on the drive.
    // The marker alone cannot authorize deleting a local path later (anyone
    // can write one to a drive); the local record is what makes cleanup of an
    // interrupted restore possible. It is best effort: without it, a partial
    // copy is simply left for the user to remove.
    let pending = PendingOperation {
        kind: PendingKind::Restore,
        original_path: plan.original_path.clone(),
        archive_path: plan.archive_path.clone(),
        started_at: Utc::now(),
    };
    let _ = journal.record(&plan.drive_root, &pending);
    if let Err(e) = PendingOperation::write(&plan.drive_root, &pending) {
        let _ = journal.forget_drive(&plan.drive_root);
        return Err(e);
    }

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

    // The local copy is now complete. Clear the marker before removing the
    // archive copy: if that removal is interrupted, a leftover marker must
    // not offer to "clean up" by deleting the fully restored local files.
    PendingOperation::clear(&plan.drive_root)?;
    let _ = journal.forget_drive(&plan.drive_root);

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

    Ok(())
}

/// Check that the archive on the drive holds exactly what the manifest says,
/// and return the location of each listed file (in `entry.checksums` order).
///
/// Every listed path is validated (see [`FileChecksum::path_under`]) and must
/// be a regular file. Restore copies the whole archive tree, so everything
/// else in it must be accounted for too: a file that is not in the manifest
/// was never verified, and tuck never writes symlinks or special files to the
/// drive. The macOS metadata files that hashing skips are tolerated.
fn checked_archive_files(archive_path: &Path, entry: &ArchiveEntry) -> TuckResult<Vec<PathBuf>> {
    let mismatch = |path: &Path, reason: &str| TuckError::UnsafePath {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    };
    let root_type = fs::symlink_metadata(archive_path)
        .io_context(archive_path)?
        .file_type();
    if entry.is_directory && !root_type.is_dir() {
        return Err(mismatch(
            archive_path,
            "the manifest records a directory here, but it is not one",
        ));
    }
    if !entry.is_directory
        && (entry.checksums.len() != 1 || !entry.checksums[0].relative_path.is_empty())
    {
        return Err(TuckError::Manifest(format!(
            "entry for {} is a single file but does not list exactly one checksum",
            entry.original_path.display()
        )));
    }

    // Files are matched by identity (device + inode) rather than by name, so
    // a drive that reports names in a different Unicode normalization or case
    // than when they were archived still matches its own manifest.
    let mut files = Vec::with_capacity(entry.checksums.len());
    let mut listed = HashSet::with_capacity(entry.checksums.len());
    for cs in &entry.checksums {
        let path = cs.path_under(archive_path)?;
        let meta = fs::symlink_metadata(&path).io_context(&path)?;
        if !meta.file_type().is_file() {
            return Err(mismatch(
                &path,
                "the manifest records a file here, but it is not a regular file",
            ));
        }
        listed.insert((meta.dev(), meta.ino()));
        files.push(path);
    }

    if entry.is_directory {
        for item in WalkDir::new(archive_path).follow_links(false) {
            let item = item.map_err(walk_error(archive_path))?;
            let file_type = item.file_type();
            if file_type.is_dir() {
                continue;
            }
            if file_type.is_file() {
                if checksum::is_ignored_metadata_file(item.file_name()) {
                    continue;
                }
                let meta = item.metadata().map_err(walk_error(archive_path))?;
                if listed.contains(&(meta.dev(), meta.ino())) {
                    continue;
                }
            }
            return Err(mismatch(
                item.path(),
                "it is in the archive but is not a file listed in the manifest, \
                 so it cannot be verified; remove it from the drive and try again",
            ));
        }
    }
    Ok(files)
}

fn walk_error(fallback: &Path) -> impl Fn(walkdir::Error) -> TuckError + '_ {
    move |e| TuckError::Io {
        path: e.path().unwrap_or(fallback).to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{execute_add_with_journal, plan_add};
    use crate::manifest::FileChecksum;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    struct Fixture {
        drive_dir: TempDir,
        local: TempDir,
        journal: RestoreJournal,
    }

    impl Fixture {
        fn new() -> Self {
            let local = TempDir::new().unwrap();
            let journal = RestoreJournal::at(local.path().join("state/restore-journal.json"));
            Fixture {
                drive_dir: TempDir::new().unwrap(),
                local,
                journal,
            }
        }

        fn drive(&self) -> DriveInfo {
            DriveInfo {
                name: "TestDrive".to_string(),
                mount_path: self.drive_dir.path().to_path_buf(),
                root_path: self.drive_dir.path().to_path_buf(),
            }
        }

        fn local_path(&self, name: &str) -> PathBuf {
            self.local.path().canonicalize().unwrap().join(name)
        }

        /// Archive `path` and delete the local copy, like `tuck add`.
        fn archive(&self, path: &Path) -> PathBuf {
            let plan = plan_add(path, &self.drive(), false).unwrap();
            execute_add_with_journal(&plan, None, &self.journal).unwrap();
            copy::remove_path(path).unwrap();
            plan.archive_path
        }

        /// Create and archive a directory with two files.
        fn archived_project(&self) -> (PathBuf, PathBuf) {
            let project = self.local_path("project");
            fs::create_dir_all(project.join("sub")).unwrap();
            fs::write(project.join("a.txt"), "aaa").unwrap();
            fs::write(project.join("sub/b.txt"), "bbb").unwrap();
            let archive_path = self.archive(&project);
            (project, archive_path)
        }

        fn restore(&self, plan: &RestorePlan, keep_archive: bool, overwrite: bool) -> TuckResult<()> {
            execute_restore_with_journal(plan, keep_archive, overwrite, None, &self.journal)
        }

        fn rewrite_manifest(&self, edit: impl FnOnce(&mut Manifest)) {
            let mut manifest = Manifest::load(self.drive_dir.path()).unwrap();
            edit(&mut manifest);
            manifest.save(self.drive_dir.path()).unwrap();
        }
    }

    #[test]
    fn test_restore_round_trip() {
        let f = Fixture::new();
        let (project, archive_path) = f.archived_project();

        let plan = plan_restore(&project, &f.drive()).unwrap();
        assert!(!plan.local_exists);
        f.restore(&plan, false, false).unwrap();

        assert_eq!(fs::read_to_string(project.join("a.txt")).unwrap(), "aaa");
        assert_eq!(fs::read_to_string(project.join("sub/b.txt")).unwrap(), "bbb");
        assert!(!archive_path.exists());
        assert!(Manifest::load(f.drive_dir.path()).unwrap().entries.is_empty());
        assert!(PendingOperation::load(f.drive_dir.path()).unwrap().is_none());
    }

    #[test]
    fn test_restore_refuses_to_overwrite_existing_local_path() {
        let f = Fixture::new();
        let file = f.local_path(".zshrc");
        fs::write(&file, "archived version").unwrap();
        f.archive(&file);
        fs::write(&file, "current version").unwrap();

        let plan = plan_restore(&file, &f.drive()).unwrap();
        assert!(plan.local_exists);
        let err = f.restore(&plan, true, false).unwrap_err();
        assert!(matches!(err, TuckError::AlreadyExists(_)));
        assert_eq!(fs::read_to_string(&file).unwrap(), "current version");

        // The check is made at execution time, not taken from the plan.
        let stale_plan = RestorePlan {
            local_exists: false,
            ..plan
        };
        let err = f.restore(&stale_plan, true, false).unwrap_err();
        assert!(matches!(err, TuckError::AlreadyExists(_)));
        assert_eq!(fs::read_to_string(&file).unwrap(), "current version");

        // Explicit overwrite still works.
        f.restore(&stale_plan, true, true).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "archived version");
    }

    #[test]
    fn test_restore_refuses_manifest_path_with_dotdot() {
        let f = Fixture::new();
        let root = f.drive_dir.path();

        // An entry that displays as ".../Documents/../authorized_keys": the
        // `..` disguises where the restore would really write.
        let disguised = f.local_path("Documents/../authorized_keys");
        let real_target = f.local_path("authorized_keys");
        let on_drive = |p: &Path| root.join(p.strip_prefix("/").unwrap());
        fs::create_dir_all(on_drive(&f.local_path("Documents"))).unwrap();
        fs::write(on_drive(&real_target), "ssh-ed25519 attacker").unwrap();
        f.rewrite_manifest(|m| {
            m.add_entry(ArchiveEntry {
                original_path: disguised.clone(),
                is_directory: false,
                archived_at: Utc::now(),
                size_bytes: 20,
                checksums: vec![FileChecksum {
                    relative_path: String::new(),
                    hash: checksum::hash_file(&on_drive(&real_target)).unwrap(),
                    size_bytes: 20,
                }],
                drive_name: "TestDrive".to_string(),
            })
            .unwrap()
        });

        let err = plan_restore(&disguised, &f.drive()).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }), "{err}");
        assert!(!real_target.exists());
    }

    #[test]
    fn test_restore_refuses_symlinked_archive_location() {
        let f = Fixture::new();
        let (project, archive_path) = f.archived_project();
        let elsewhere = f.local_path("elsewhere");
        fs::rename(&archive_path, &elsewhere).unwrap();
        symlink(&elsewhere, &archive_path).unwrap();

        let err = plan_restore(&project, &f.drive()).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));
        assert!(!project.exists());
    }

    #[test]
    fn test_restore_refuses_relative_path_outside_archive() {
        let f = Fixture::new();
        let (project, _) = f.archived_project();
        let secret = f.local_path("id_rsa");
        fs::write(&secret, "PRIVATE KEY").unwrap();

        for bad in [secret.to_string_lossy().into_owned(), "../../outside".into()] {
            f.rewrite_manifest(|m| m.entries[0].checksums[0].relative_path = bad.clone());
            let plan = plan_restore(&project, &f.drive()).unwrap();
            let err = f.restore(&plan, true, false).unwrap_err();
            assert!(matches!(err, TuckError::UnsafePath { .. }), "{bad}: {err}");
            assert!(!project.exists());
        }
    }

    #[test]
    fn test_restore_refuses_files_not_in_manifest() {
        let f = Fixture::new();
        let (project, archive_path) = f.archived_project();
        fs::write(archive_path.join("sub/planted.sh"), "#!/bin/sh").unwrap();

        let plan = plan_restore(&project, &f.drive()).unwrap();
        let err = f.restore(&plan, true, false).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));
        assert!(!project.exists());

        // macOS metadata files are not in the manifest and are still fine.
        fs::remove_file(archive_path.join("sub/planted.sh")).unwrap();
        fs::write(archive_path.join(".DS_Store"), "x").unwrap();
        fs::write(archive_path.join("sub/._b.txt"), "x").unwrap();
        f.restore(&plan, true, false).unwrap();
        assert_eq!(fs::read_to_string(project.join("sub/b.txt")).unwrap(), "bbb");
    }

    #[test]
    fn test_restore_refuses_symlink_inside_archive() {
        let f = Fixture::new();
        let (project, archive_path) = f.archived_project();
        symlink("/etc", archive_path.join("link")).unwrap();

        let plan = plan_restore(&project, &f.drive()).unwrap();
        let err = f.restore(&plan, true, false).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));
        assert!(!project.exists());
    }

    #[test]
    fn test_restore_detects_corrupted_archive() {
        let f = Fixture::new();
        let (project, archive_path) = f.archived_project();
        fs::write(archive_path.join("a.txt"), "tampered").unwrap();

        let plan = plan_restore(&project, &f.drive()).unwrap();
        let err = f.restore(&plan, true, false).unwrap_err();
        assert!(matches!(err, TuckError::ChecksumMismatch { .. }));
        assert!(!project.exists());
    }

    #[test]
    fn test_interrupted_restore_is_cleaned_up_only_with_local_record() {
        let f = Fixture::new();
        let (project, archive_path) = f.archived_project();
        let root = f.drive_dir.path();

        // Simulate an interruption mid-copy: marker and journal written, a
        // partial local copy on disk, manifest entry still present.
        let pending = PendingOperation {
            kind: PendingKind::Restore,
            original_path: project.clone(),
            archive_path: archive_path.clone(),
            started_at: Utc::now(),
        };
        f.journal.record(root, &pending).unwrap();
        PendingOperation::write(root, &pending).unwrap();
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("a.txt"), "a").unwrap();

        let status = PendingOperation::inspect_with_journal(root, &f.journal)
            .unwrap()
            .unwrap();
        status.resolve_with_journal(root, &f.journal).unwrap();

        assert!(!project.exists());
        assert!(archive_path.join("a.txt").exists());

        // The archive is intact, so the restore can simply be run again.
        let plan = plan_restore(&project, &f.drive()).unwrap();
        f.restore(&plan, false, false).unwrap();
        assert_eq!(fs::read_to_string(project.join("a.txt")).unwrap(), "aaa");
    }

    #[test]
    fn test_marker_is_cleared_before_archive_is_removed() {
        use std::cell::Cell;

        /// Records whether anything is still pending at the moment the
        /// "Removing archive copy" phase begins.
        struct Probe<'a> {
            fixture: &'a Fixture,
            pending_at_removal: Cell<Option<bool>>,
        }
        impl Progress for Probe<'_> {
            fn start_phase(&self, phase: &str, _total_bytes: u64) {
                if phase == "Removing archive copy" {
                    let root = self.fixture.drive_dir.path();
                    let pending =
                        PendingOperation::inspect_with_journal(root, &self.fixture.journal)
                            .unwrap()
                            .is_some();
                    self.pending_at_removal.set(Some(pending));
                }
            }
            fn advance(&self, _bytes: u64) {}
            fn finish_phase(&self) {}
        }

        let f = Fixture::new();
        let (project, archive_path) = f.archived_project();
        let plan = plan_restore(&project, &f.drive()).unwrap();
        let probe = Probe {
            fixture: &f,
            pending_at_removal: Cell::new(None),
        };
        execute_restore_with_journal(&plan, false, false, Some(&probe), &f.journal).unwrap();

        // If the removal is interrupted, nothing is left pending, so no later
        // cleanup can be offered that would delete the restored local copy.
        assert_eq!(probe.pending_at_removal.get(), Some(false));
        assert_eq!(fs::read_to_string(project.join("a.txt")).unwrap(), "aaa");
        assert!(!archive_path.exists());
    }
}

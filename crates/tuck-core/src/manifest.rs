use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{TuckError, TuckResult};
use crate::safety;

const MANIFEST_FILENAME: &str = ".tuck-manifest.json";
/// Upper bound on the manifest file (roughly four million archived files).
/// Any mounted volume can carry a file under the manifest's name; without a
/// bound, a huge one would be read into memory in full.
const MANIFEST_MAX_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChecksum {
    pub relative_path: String,
    pub hash: String,
    pub size_bytes: u64,
}

impl FileChecksum {
    /// Location of this file under `archive_base` (the entry's archive path).
    ///
    /// `relative_path` comes from the manifest on the drive, so it is checked
    /// rather than trusted: it must be empty (the entry is a single file) or
    /// a plain relative path, and nothing along it may be a symlink. An
    /// absolute path or `..` would otherwise point tuck at files outside the
    /// archive.
    pub fn path_under(&self, archive_base: &Path) -> TuckResult<PathBuf> {
        if self.relative_path.is_empty() {
            return Ok(archive_base.to_path_buf());
        }
        let rel = Path::new(&self.relative_path);
        if !safety::is_plain_relative(rel) {
            return Err(TuckError::UnsafePath {
                path: rel.to_path_buf(),
                reason: "the manifest lists it as a file inside an archive, \
                         but it is not a plain relative path"
                    .to_string(),
            });
        }
        let path = archive_base.join(rel);
        safety::ensure_contained(archive_base, &path)?;
        Ok(path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub original_path: PathBuf,
    pub is_directory: bool,
    pub archived_at: DateTime<Utc>,
    pub size_bytes: u64,
    pub checksums: Vec<FileChecksum>,
    pub drive_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub entries: Vec<ArchiveEntry>,
}

impl Manifest {
    pub fn new() -> Self {
        Manifest {
            version: 1,
            entries: Vec::new(),
        }
    }

    /// Returns the path to the manifest file on the given drive mount point.
    pub fn path_on_drive(drive_mount: &Path) -> PathBuf {
        drive_mount.join(MANIFEST_FILENAME)
    }

    /// Load manifest from disk. Returns a new empty manifest if file doesn't exist.
    ///
    /// The manifest must be a regular file; a symlink planted under its name
    /// is refused rather than followed.
    pub fn load(drive_mount: &Path) -> TuckResult<Self> {
        let path = Self::path_on_drive(drive_mount);
        match safety::read_metadata_file(&path, Some(MANIFEST_MAX_BYTES))? {
            Some(data) => {
                serde_json::from_str(&data).map_err(|e| TuckError::Manifest(e.to_string()))
            }
            None => Ok(Self::new()),
        }
    }

    /// Save manifest atomically (write to .tmp, then rename), without
    /// following a symlink at either name.
    pub fn save(&self, drive_mount: &Path) -> TuckResult<()> {
        let path = Self::path_on_drive(drive_mount);
        let data =
            serde_json::to_string_pretty(self).map_err(|e| TuckError::Manifest(e.to_string()))?;
        // Never write a manifest that `load` would refuse to read back.
        if data.len() as u64 > MANIFEST_MAX_BYTES {
            return Err(TuckError::Manifest(format!(
                "the manifest would exceed {MANIFEST_MAX_BYTES} bytes; \
                 use a separate --prefix for further archives"
            )));
        }
        safety::write_metadata_file(&path, data.as_bytes())
    }

    /// Entries whose original path lies strictly inside `path`. Their archives
    /// are stored inside the archive tree of `path` on the drive.
    pub fn entries_under<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Iterator<Item = &'a ArchiveEntry> + 'a {
        self.entries
            .iter()
            .filter(move |e| e.original_path != path && e.original_path.starts_with(path))
    }

    /// Find an entry by its original path (canonicalized).
    pub fn find_entry(&self, original_path: &Path) -> Option<&ArchiveEntry> {
        self.entries.iter().find(|e| e.original_path == original_path)
    }

    /// Add an entry. Returns error if the path is already archived.
    pub fn add_entry(&mut self, entry: ArchiveEntry) -> TuckResult<()> {
        if self.find_entry(&entry.original_path).is_some() {
            return Err(TuckError::AlreadyExists(entry.original_path));
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Remove an entry by original path. Returns the removed entry or NotArchived error.
    pub fn remove_entry(&mut self, original_path: &Path) -> TuckResult<ArchiveEntry> {
        let idx = self
            .entries
            .iter()
            .position(|e| e.original_path == original_path)
            .ok_or_else(|| TuckError::NotArchived(original_path.to_path_buf()))?;
        Ok(self.entries.remove(idx))
    }
}

impl Default for Manifest {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn sample_entry(path: &str) -> ArchiveEntry {
        ArchiveEntry {
            original_path: PathBuf::from(path),
            is_directory: false,
            archived_at: Utc::now(),
            size_bytes: 1024,
            checksums: vec![FileChecksum {
                relative_path: String::new(),
                hash: "abc123".to_string(),
                size_bytes: 1024,
            }],
            drive_name: "TestDrive".to_string(),
        }
    }

    #[test]
    fn test_new_manifest() {
        let m = Manifest::new();
        assert_eq!(m.version, 1);
        assert!(m.entries.is_empty());
    }

    #[test]
    fn test_add_and_find_entry() {
        let mut m = Manifest::new();
        m.add_entry(sample_entry("/Users/test/file.txt")).unwrap();
        assert!(m.find_entry(Path::new("/Users/test/file.txt")).is_some());
        assert!(m.find_entry(Path::new("/Users/test/other.txt")).is_none());
    }

    #[test]
    fn test_add_duplicate_entry() {
        let mut m = Manifest::new();
        m.add_entry(sample_entry("/Users/test/file.txt")).unwrap();
        let result = m.add_entry(sample_entry("/Users/test/file.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_entry() {
        let mut m = Manifest::new();
        m.add_entry(sample_entry("/Users/test/file.txt")).unwrap();
        let removed = m.remove_entry(Path::new("/Users/test/file.txt")).unwrap();
        assert_eq!(removed.original_path, PathBuf::from("/Users/test/file.txt"));
        assert!(m.entries.is_empty());
    }

    #[test]
    fn test_remove_nonexistent_entry() {
        let mut m = Manifest::new();
        let result = m.remove_entry(Path::new("/Users/test/nope.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn test_save_and_load() {
        let dir = TempDir::new().unwrap();
        let mut m = Manifest::new();
        m.add_entry(sample_entry("/Users/test/file.txt")).unwrap();
        m.save(dir.path()).unwrap();

        let loaded = Manifest::load(dir.path()).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(
            loaded.entries[0].original_path,
            PathBuf::from("/Users/test/file.txt")
        );
    }

    #[test]
    fn test_load_nonexistent_returns_empty() {
        let dir = TempDir::new().unwrap();
        let m = Manifest::load(dir.path()).unwrap();
        assert!(m.entries.is_empty());
    }

    #[test]
    fn test_load_refuses_symlinked_manifest() {
        let dir = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();
        let target = elsewhere.path().join("other.json");
        fs::write(&target, r#"{"version":1,"entries":[]}"#).unwrap();
        symlink(&target, Manifest::path_on_drive(dir.path())).unwrap();

        let err = Manifest::load(dir.path()).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));
    }

    #[test]
    fn test_load_refuses_oversized_manifest() {
        let dir = TempDir::new().unwrap();
        // A sparse file: large on paper, instant to create.
        let file = fs::File::create(Manifest::path_on_drive(dir.path())).unwrap();
        file.set_len(MANIFEST_MAX_BYTES + 1).unwrap();

        let err = Manifest::load(dir.path()).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));
    }

    #[test]
    fn test_entries_under() {
        let mut m = Manifest::new();
        for path in ["/Users/me/Documents", "/Users/me/Documents/proj", "/Users/me/Documents2"] {
            m.add_entry(sample_entry(path)).unwrap();
        }
        let under: Vec<_> = m
            .entries_under(Path::new("/Users/me/Documents"))
            .map(|e| e.original_path.clone())
            .collect();
        assert_eq!(under, vec![PathBuf::from("/Users/me/Documents/proj")]);
        assert_eq!(m.entries_under(Path::new("/Users/me/Documents/proj")).count(), 0);
    }

    #[test]
    fn test_save_does_not_write_through_planted_symlinks() {
        let dir = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();
        let victim = elsewhere.path().join(".zshrc");
        fs::write(&victim, "export PATH=...").unwrap();
        // Planted at the temp name used during the atomic write.
        symlink(&victim, dir.path().join(".tuck-manifest.json.tmp")).unwrap();

        let mut m = Manifest::new();
        m.add_entry(sample_entry("/Users/test/file.txt")).unwrap();
        m.save(dir.path()).unwrap();

        assert_eq!(fs::read_to_string(&victim).unwrap(), "export PATH=...");
        assert_eq!(Manifest::load(dir.path()).unwrap().entries.len(), 1);
    }

    #[test]
    fn test_checksum_path_under() {
        let base = TempDir::new().unwrap();
        let cs = |rel: &str| FileChecksum {
            relative_path: rel.to_string(),
            hash: String::new(),
            size_bytes: 0,
        };

        assert_eq!(cs("").path_under(base.path()).unwrap(), base.path());
        assert_eq!(
            cs("sub/b.txt").path_under(base.path()).unwrap(),
            base.path().join("sub/b.txt")
        );
        for bad in ["/etc/passwd", "../outside", "sub/../../outside"] {
            let err = cs(bad).path_under(base.path()).unwrap_err();
            assert!(matches!(err, TuckError::UnsafePath { .. }), "{bad}");
        }

        // A symlink inside the archive is not followed either.
        symlink("/etc", base.path().join("link")).unwrap();
        let err = cs("link/passwd").path_under(base.path()).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));
    }
}

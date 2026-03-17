use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{IoContext, TuckError, TuckResult};

const MANIFEST_FILENAME: &str = ".tuck-manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChecksum {
    pub relative_path: String,
    pub hash: String,
    pub size_bytes: u64,
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
    pub fn load(drive_mount: &Path) -> TuckResult<Self> {
        let path = Self::path_on_drive(drive_mount);
        if !path.exists() {
            return Ok(Self::new());
        }
        let data = fs::read_to_string(&path).io_context(&path)?;
        serde_json::from_str(&data).map_err(|e| TuckError::Manifest(e.to_string()))
    }

    /// Save manifest atomically (write to .tmp, then rename).
    pub fn save(&self, drive_mount: &Path) -> TuckResult<()> {
        let path = Self::path_on_drive(drive_mount);
        let tmp_path = path.with_extension("json.tmp");
        let data =
            serde_json::to_string_pretty(self).map_err(|e| TuckError::Manifest(e.to_string()))?;
        fs::write(&tmp_path, &data).io_context(&tmp_path)?;
        fs::rename(&tmp_path, &path).io_context(&path)?;
        Ok(())
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
}

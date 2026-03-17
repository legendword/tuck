use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{IoContext, TuckError, TuckResult};

const VOLUMES_DIR: &str = "/Volumes";

/// Information about a detected external drive.
#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub name: String,
    pub mount_path: PathBuf,
    /// Effective root for tuck operations. Equals `mount_path` by default,
    /// or `mount_path/prefix` when a prefix subfolder is specified.
    pub root_path: PathBuf,
}

impl DriveInfo {
    /// Return a new DriveInfo with the given prefix applied to root_path.
    pub fn with_prefix(mut self, prefix: Option<&str>) -> Self {
        if let Some(p) = prefix {
            if !p.is_empty() {
                self.root_path = self.mount_path.join(p);
            }
        }
        self
    }
}

/// List all external drives by scanning /Volumes/.
/// Skips symlinks to "/" (the boot volume) and hidden entries.
pub fn list_drives() -> TuckResult<Vec<DriveInfo>> {
    let volumes = Path::new(VOLUMES_DIR);
    if !volumes.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(volumes).map_err(|e| TuckError::Io {
        source: e,
        path: volumes.to_path_buf(),
    })?;

    let mut drives = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| TuckError::Io {
            source: e,
            path: volumes.to_path_buf(),
        })?;
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden entries
        if name.starts_with('.') {
            continue;
        }

        let path = entry.path();

        // Skip symlinks to "/" (boot volume)
        if path.is_symlink() {
            if let Ok(target) = fs::read_link(&path) {
                if target == Path::new("/") {
                    continue;
                }
            }
        }

        // Only include directories
        if path.is_dir() {
            drives.push(DriveInfo {
                name,
                root_path: path.clone(),
                mount_path: path,
            });
        }
    }
    Ok(drives)
}

/// Find a specific drive by name.
pub fn find_drive(name: &str) -> TuckResult<DriveInfo> {
    let drives = list_drives()?;
    drives
        .into_iter()
        .find(|d| d.name == name)
        .ok_or_else(|| TuckError::DriveNotConnected(name.to_string()))
}

/// Auto-detect a single external drive. Errors if zero or multiple drives found.
pub fn auto_detect_drive() -> TuckResult<DriveInfo> {
    let drives = list_drives()?;
    match drives.len() {
        0 => Err(TuckError::NoDriveFound),
        1 => Ok(drives.into_iter().next().unwrap()),
        _ => {
            let names: Vec<String> = drives.iter().map(|d| d.name.clone()).collect();
            Err(TuckError::MultipleDrivesFound(names))
        }
    }
}

/// Resolve a drive from an optional name and prefix.
/// If name is provided, find it; otherwise auto-detect.
/// If prefix is provided, root_path is set to mount_path/prefix.
pub fn resolve_drive(name: Option<&str>, prefix: Option<&str>) -> TuckResult<DriveInfo> {
    let drive = match name {
        Some(n) => find_drive(n),
        None => auto_detect_drive(),
    }?;
    Ok(drive.with_prefix(prefix))
}

/// Query available disk space (in bytes) for the filesystem containing `path`.
pub fn available_space(path: &Path) -> TuckResult<u64> {
    fs2::available_space(path).io_context(path)
}

/// Check that `path` has at least `needed_bytes` of free space.
/// Returns `Ok(())` if sufficient, or `Err(InsufficientSpace)` otherwise.
pub fn check_space(path: &Path, needed_bytes: u64) -> TuckResult<()> {
    let available = available_space(path)?;
    if available < needed_bytes {
        return Err(TuckError::InsufficientSpace {
            path: path.to_path_buf(),
            needed: humanize_bytes(needed_bytes),
            available: humanize_bytes(available),
        });
    }
    Ok(())
}

fn humanize_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Compute the archive path on the drive for a given original (canonicalized) path.
/// Strips leading "/" and joins with the drive mount path.
/// e.g., `/Users/foo/bar.txt` -> `/Volumes/Drive/Users/foo/bar.txt`
pub fn archive_path_on_drive(drive_mount: &Path, original_path: &Path) -> PathBuf {
    let stripped = original_path
        .strip_prefix("/")
        .unwrap_or(original_path);
    drive_mount.join(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archive_path_on_drive() {
        let drive = Path::new("/Volumes/MyDrive");
        let original = Path::new("/Users/foo/Documents/project");
        let result = archive_path_on_drive(drive, original);
        assert_eq!(result, PathBuf::from("/Volumes/MyDrive/Users/foo/Documents/project"));
    }

    #[test]
    fn test_archive_path_on_drive_file() {
        let drive = Path::new("/Volumes/MyDrive");
        let original = Path::new("/Users/foo/file.txt");
        let result = archive_path_on_drive(drive, original);
        assert_eq!(result, PathBuf::from("/Volumes/MyDrive/Users/foo/file.txt"));
    }

    #[test]
    fn test_archive_path_with_prefix() {
        let root = Path::new("/Volumes/MyDrive/tuck-macbook");
        let original = Path::new("/Users/foo/file.txt");
        let result = archive_path_on_drive(root, original);
        assert_eq!(result, PathBuf::from("/Volumes/MyDrive/tuck-macbook/Users/foo/file.txt"));
    }

    #[test]
    fn test_with_prefix() {
        let drive = DriveInfo {
            name: "MyDrive".to_string(),
            mount_path: PathBuf::from("/Volumes/MyDrive"),
            root_path: PathBuf::from("/Volumes/MyDrive"),
        };
        let prefixed = drive.with_prefix(Some("tuck-macbook"));
        assert_eq!(prefixed.mount_path, PathBuf::from("/Volumes/MyDrive"));
        assert_eq!(prefixed.root_path, PathBuf::from("/Volumes/MyDrive/tuck-macbook"));
    }

    #[test]
    fn test_check_space_sufficient() {
        // Current directory should have plenty of space for 1 byte
        let tmp = tempfile::tempdir().unwrap();
        assert!(check_space(tmp.path(), 1).is_ok());
    }

    #[test]
    fn test_check_space_insufficient() {
        // Request an absurdly large amount of space
        let tmp = tempfile::tempdir().unwrap();
        let result = check_space(tmp.path(), u64::MAX);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, TuckError::InsufficientSpace { .. }));
    }

    #[test]
    fn test_humanize_bytes() {
        assert_eq!(humanize_bytes(500), "500 B");
        assert_eq!(humanize_bytes(1024), "1.0 KiB");
        assert_eq!(humanize_bytes(1536), "1.5 KiB");
        assert_eq!(humanize_bytes(1048576), "1.0 MiB");
        assert_eq!(humanize_bytes(1073741824), "1.0 GiB");
    }

    #[test]
    fn test_with_no_prefix() {
        let drive = DriveInfo {
            name: "MyDrive".to_string(),
            mount_path: PathBuf::from("/Volumes/MyDrive"),
            root_path: PathBuf::from("/Volumes/MyDrive"),
        };
        let same = drive.with_prefix(None);
        assert_eq!(same.root_path, PathBuf::from("/Volumes/MyDrive"));
    }
}

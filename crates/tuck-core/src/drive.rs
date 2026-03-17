use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{TuckError, TuckResult};

const VOLUMES_DIR: &str = "/Volumes";

/// Information about a detected external drive.
#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub name: String,
    pub mount_path: PathBuf,
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

/// Resolve a drive from an optional name: if provided, find it; otherwise auto-detect.
pub fn resolve_drive(name: Option<&str>) -> TuckResult<DriveInfo> {
    match name {
        Some(n) => find_drive(n),
        None => auto_detect_drive(),
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
}

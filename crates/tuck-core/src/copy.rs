use std::fs;
use std::path::Path;

use filetime::FileTime;
use walkdir::WalkDir;

use crate::error::{IoContext, TuckError, TuckResult};
use crate::progress::Progress;

/// Recursively copy a file or directory from `src` to `dst`, preserving modification times.
/// Symlinks within directories are skipped with a warning.
/// The parent directory of `dst` must exist.
pub fn copy_recursive(
    src: &Path,
    dst: &Path,
    progress: Option<&dyn Progress>,
) -> TuckResult<()> {
    if src.is_file() {
        copy_file_with_metadata(src, dst)?;
        if let Some(p) = progress {
            let size = fs::metadata(src).io_context(src)?.len();
            p.advance(size);
        }
    } else if src.is_dir() {
        copy_dir_recursive(src, dst, progress)?;
    } else {
        return Err(TuckError::PathNotFound(src.to_path_buf()));
    }
    Ok(())
}

/// Copy a single file and preserve its modification time.
fn copy_file_with_metadata(src: &Path, dst: &Path) -> TuckResult<()> {
    // Ensure parent directory exists
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).io_context(parent)?;
    }

    fs::copy(src, dst).io_context(src)?;

    // Preserve modification time
    let src_meta = fs::metadata(src).io_context(src)?;
    let mtime = FileTime::from_last_modification_time(&src_meta);
    filetime::set_file_mtime(dst, mtime).io_context(dst)?;

    Ok(())
}

/// Recursively copy a directory.
fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    progress: Option<&dyn Progress>,
) -> TuckResult<()> {
    for entry in WalkDir::new(src).follow_links(false).sort_by_file_name() {
        let entry = entry.map_err(|e| {
            let path = e.path().unwrap_or(src).to_path_buf();
            TuckError::Io {
                source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                path,
            }
        })?;

        let rel = entry
            .path()
            .strip_prefix(src)
            .expect("entry must be under src");
        let target = dst.join(rel);

        if entry.file_type().is_symlink() {
            eprintln!(
                "warning: skipping symlink: {}",
                entry.path().display()
            );
            continue;
        }

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).io_context(&target)?;
            // Preserve directory modification time
            let meta = fs::metadata(entry.path()).io_context(entry.path())?;
            let mtime = FileTime::from_last_modification_time(&meta);
            filetime::set_file_mtime(&target, mtime).io_context(&target)?;
        } else if entry.file_type().is_file() {
            copy_file_with_metadata(entry.path(), &target)?;
            if let Some(p) = progress {
                let size = entry.metadata().map_err(|e| TuckError::Io {
                    source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                    path: entry.path().to_path_buf(),
                })?.len();
                p.advance(size);
            }
        }
    }
    Ok(())
}

/// Remove a file or directory recursively.
pub fn remove_path(path: &Path) -> TuckResult<()> {
    if path.is_dir() {
        fs::remove_dir_all(path).io_context(path)
    } else {
        fs::remove_file(path).io_context(path)
    }
}

/// Calculate total size of a path (file or directory) in bytes.
pub fn path_size(path: &Path) -> TuckResult<u64> {
    if path.is_file() {
        let meta = fs::metadata(path).io_context(path)?;
        Ok(meta.len())
    } else if path.is_dir() {
        let mut total = 0u64;
        for entry in WalkDir::new(path).follow_links(false) {
            let entry = entry.map_err(|e| {
                let p = e.path().unwrap_or(path).to_path_buf();
                TuckError::Io {
                    source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                    path: p,
                }
            })?;
            if entry.file_type().is_file() {
                let meta = entry.metadata().map_err(|e| TuckError::Io {
                    source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                    path: entry.path().to_path_buf(),
                })?;
                total += meta.len();
            }
        }
        Ok(total)
    } else {
        Err(TuckError::PathNotFound(path.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_copy_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("dest.txt");
        fs::write(&src, "hello world").unwrap();

        copy_recursive(&src, &dst, None).unwrap();

        assert_eq!(fs::read_to_string(&dst).unwrap(), "hello world");

        // Verify modification time preserved
        let src_meta = fs::metadata(&src).unwrap();
        let dst_meta = fs::metadata(&dst).unwrap();
        assert_eq!(
            FileTime::from_last_modification_time(&src_meta),
            FileTime::from_last_modification_time(&dst_meta)
        );
    }

    #[test]
    fn test_copy_directory() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("srcdir");
        let dst = dir.path().join("dstdir");

        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.txt"), "aaa").unwrap();
        fs::create_dir(src.join("sub")).unwrap();
        fs::write(src.join("sub/b.txt"), "bbb").unwrap();

        copy_recursive(&src, &dst, None).unwrap();

        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "aaa");
        assert_eq!(fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "bbb");
    }

    #[test]
    fn test_remove_path_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "data").unwrap();
        assert!(file.exists());
        remove_path(&file).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn test_remove_path_dir() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("file.txt"), "data").unwrap();
        assert!(subdir.exists());
        remove_path(&subdir).unwrap();
        assert!(!subdir.exists());
    }

    #[test]
    fn test_path_size_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "12345").unwrap();
        assert_eq!(path_size(&file).unwrap(), 5);
    }

    #[test]
    fn test_path_size_dir() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("sub");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("a.txt"), "aaa").unwrap();
        fs::write(subdir.join("b.txt"), "bb").unwrap();
        assert_eq!(path_size(&subdir).unwrap(), 5);
    }
}

use std::fs::File;
use std::io::Read;
use std::path::Path;

use walkdir::WalkDir;

use crate::error::{IoContext, TuckResult};
use crate::manifest::FileChecksum;
use crate::progress::Progress;

const CHUNK_SIZE: usize = 64 * 1024; // 64 KB

/// Compute BLAKE3 hash of a single file using streaming reads.
pub fn hash_file(path: &Path) -> TuckResult<String> {
    let mut file = File::open(path).io_context(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = file.read(&mut buf).io_context(path)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Compute checksums for a directory, returning a FileChecksum per regular file.
/// Symlinks are skipped. Paths are relative to `dir`.
pub fn hash_directory(
    dir: &Path,
    progress: Option<&dyn Progress>,
) -> TuckResult<Vec<FileChecksum>> {
    let mut checksums = Vec::new();
    for entry in WalkDir::new(dir).follow_links(false).sort_by_file_name() {
        let entry = entry.map_err(|e| {
            let path = e.path().unwrap_or(dir).to_path_buf();
            crate::error::TuckError::Io {
                source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                path,
            }
        })?;
        let ft = entry.file_type();
        if ft.is_symlink() {
            eprintln!(
                "warning: skipping symlink: {}",
                entry.path().display()
            );
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(dir)
            .expect("entry must be under dir")
            .to_string_lossy()
            .to_string();
        let hash = hash_file(entry.path())?;
        let size = entry.metadata().map_err(|e| crate::error::TuckError::Io {
            source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            path: entry.path().to_path_buf(),
        })?.len();
        checksums.push(FileChecksum {
            relative_path: rel,
            hash,
            size_bytes: size,
        });
        if let Some(p) = progress {
            p.advance(size);
        }
    }
    Ok(checksums)
}

/// Hash a path — delegates to hash_file or hash_directory.
pub fn hash_path(path: &Path, progress: Option<&dyn Progress>) -> TuckResult<Vec<FileChecksum>> {
    if path.is_dir() {
        hash_directory(path, progress)
    } else {
        let meta = std::fs::metadata(path).io_context(path)?;
        let hash = hash_file(path)?;
        let size = meta.len();
        if let Some(p) = progress {
            p.advance(size);
        }
        Ok(vec![FileChecksum {
            relative_path: String::new(),
            hash,
            size_bytes: size,
        }])
    }
}

/// Verify that a file on disk matches an expected checksum.
pub fn verify_checksum(file_path: &Path, expected: &str) -> TuckResult<bool> {
    let actual = hash_file(file_path)?;
    Ok(actual == expected)
}

/// Verify a checksum and report progress.
pub fn verify_checksum_with_progress(
    file_path: &Path,
    expected: &str,
    size: u64,
    progress: Option<&dyn Progress>,
) -> TuckResult<bool> {
    let actual = hash_file(file_path)?;
    if let Some(p) = progress {
        p.advance(size);
    }
    Ok(actual == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_hash_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world").unwrap();

        let hash1 = hash_file(&file).unwrap();
        let hash2 = hash_file(&file).unwrap();
        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());
    }

    #[test]
    fn test_hash_file_different_content() {
        let dir = TempDir::new().unwrap();
        let f1 = dir.path().join("a.txt");
        let f2 = dir.path().join("b.txt");
        fs::write(&f1, "hello").unwrap();
        fs::write(&f2, "world").unwrap();

        assert_ne!(hash_file(&f1).unwrap(), hash_file(&f2).unwrap());
    }

    #[test]
    fn test_hash_directory() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "aaa").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.txt"), "bbb").unwrap();

        let checksums = hash_directory(dir.path(), None).unwrap();
        assert_eq!(checksums.len(), 2);

        let paths: Vec<&str> = checksums.iter().map(|c| c.relative_path.as_str()).collect();
        assert!(paths.contains(&"a.txt"));
        assert!(paths.contains(&"sub/b.txt"));
    }

    #[test]
    fn test_hash_path_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "content").unwrap();

        let checksums = hash_path(&file, None).unwrap();
        assert_eq!(checksums.len(), 1);
        assert_eq!(checksums[0].relative_path, "");
    }

    #[test]
    fn test_verify_checksum() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello").unwrap();

        let hash = hash_file(&file).unwrap();
        assert!(verify_checksum(&file, &hash).unwrap());
        assert!(!verify_checksum(&file, "wrong_hash").unwrap());
    }
}

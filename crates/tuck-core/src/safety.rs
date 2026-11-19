//! Helpers for handling paths and metadata files that live on the drive.
//!
//! Everything on the external drive is untrusted input: the manifest, the
//! pending marker, and the directory layout can all be authored or modified by
//! someone else (another machine, another user, a crafted disk image). The
//! functions here make sure that a path derived from that data stays inside
//! the tuck root and that tuck never reads or writes *through* a symlink that
//! was planted on the drive.
//!
//! These are check-then-use guards. They defend against a drive that was
//! prepared in advance; they are not a defence against another process that
//! rewrites the drive while tuck is running.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use crate::error::{IoContext, TuckError, TuckResult};

/// Make text that comes from the drive safe to show to the user.
///
/// Manifest entries, marker contents and volume names are written by
/// whoever last had the drive. Control characters (including the ESC that
/// starts a terminal escape sequence, and newlines) and Unicode
/// bidirectional overrides are shown in escaped form, so stored text cannot
/// move the cursor, restyle the screen, or disguise a path in a prompt.
pub fn printable(value: impl std::fmt::Display) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let bidi_control = matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        );
        if c.is_control() || bidi_control {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
}

fn unsafe_path(path: &Path, reason: impl Into<String>) -> TuckError {
    TuckError::UnsafePath {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

/// True if `path` is a non-empty relative path made only of plain names
/// (no root, no `..`, no `.`).
pub fn is_plain_relative(path: &Path) -> bool {
    let mut any = false;
    for c in path.components() {
        match c {
            Component::Normal(_) => any = true,
            _ => return false,
        }
    }
    any
}

/// True if `path` is absolute and, below the root, made only of plain names
/// (no `..`). This is the shape of every path tuck writes to a manifest,
/// because those are canonicalized first.
pub fn is_plain_absolute(path: &Path) -> bool {
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return false;
    }
    components.all(|c| matches!(c, Component::Normal(_)))
}

/// Verify that `path` is strictly inside `root` and can be used without
/// leaving it:
///
/// * `path` must be `root` plus at least one plain name (no `..`), and
/// * no existing component below `root`, including `path` itself, may be a
///   symlink.
///
/// `root` itself is taken as given (it is the mount point or prefix folder
/// the user chose). Components that do not exist yet are fine: nothing can be
/// followed there.
pub fn ensure_contained(root: &Path, path: &Path) -> TuckResult<()> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| unsafe_path(path, format!("it is outside {}", root.display())))?;
    if !is_plain_relative(rel) {
        return Err(unsafe_path(
            path,
            format!("it is not a plain path inside {}", root.display()),
        ));
    }

    let mut current = root.to_path_buf();
    for component in rel.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(unsafe_path(
                    path,
                    format!("{} is a symlink", current.display()),
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => break,
            Err(e) => return Err(e).io_context(&current),
        }
    }
    Ok(())
}

/// Read a metadata file from the drive.
///
/// Returns `Ok(None)` if the file does not exist. Refuses anything that is
/// not a regular file, never follows a symlink at `path`, and (when
/// `max_bytes` is given) refuses files larger than that.
pub fn read_metadata_file(path: &Path, max_bytes: Option<u64>) -> TuckResult<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return Err(unsafe_path(path, "it is not a regular file")),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).io_context(path),
    }

    // O_NOFOLLOW closes the window between the check above and the open.
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .io_context(path)?;
    let meta = file.metadata().io_context(path)?;
    if !meta.is_file() {
        return Err(unsafe_path(path, "it is not a regular file"));
    }
    if let Some(max) = max_bytes {
        if meta.len() > max {
            return Err(unsafe_path(path, format!("it is larger than {max} bytes")));
        }
    }

    let mut bytes = Vec::new();
    match max_bytes {
        Some(max) => (&mut file).take(max).read_to_end(&mut bytes),
        None => file.read_to_end(&mut bytes),
    }
    .io_context(path)?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| unsafe_path(path, "it is not valid UTF-8 text"))
}

/// Path of the temporary sibling used by [`write_metadata_file`].
pub fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

/// Atomically write a metadata file to the drive.
///
/// The data goes to a freshly created sibling (`<name>.tmp`, opened with
/// `O_EXCL` so a planted symlink is never followed) which is then renamed over
/// `path`. `rename` replaces a symlink at `path` rather than following it.
pub fn write_metadata_file(path: &Path, data: &[u8]) -> TuckResult<()> {
    let tmp = tmp_sibling(path);
    // A leftover temp file (or a symlink planted under that name) is removed,
    // not reused. `remove_file` unlinks a symlink itself.
    remove_file_if_present(&tmp)?;

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .io_context(&tmp)?;
    let written = file
        .write_all(data)
        .and_then(|_| file.sync_all())
        .io_context(&tmp);
    drop(file);
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    fs::rename(&tmp, path).io_context(path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e
    })
}

/// Remove a file if it exists. A symlink is unlinked, never followed.
pub fn remove_file_if_present(path: &Path) -> TuckResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).io_context(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    #[test]
    fn test_printable_escapes_terminal_control_sequences() {
        // ESC [ 2 K (erase line) and a carriage return, as a path might carry.
        let shown = printable("/Users/me/\u{1b}[2K\rDocuments");
        assert!(!shown.chars().any(|c| c.is_control()));
        assert_eq!(shown, "/Users/me/\\u{1b}[2K\\u{d}Documents");
    }

    #[test]
    fn test_printable_escapes_newlines_and_bidi_overrides() {
        assert_eq!(printable("a\nb\u{202E}txt.exe"), "a\\u{a}b\\u{202e}txt.exe");
    }

    #[test]
    fn test_printable_leaves_ordinary_text_alone() {
        let plain = "/Users/me/Documents/Résumé – final (2).pdf";
        assert_eq!(printable(plain), plain);
    }

    #[test]
    fn test_plain_relative() {
        assert!(is_plain_relative(Path::new("a")));
        assert!(is_plain_relative(Path::new("a/b/c.txt")));
        assert!(!is_plain_relative(Path::new("")));
        assert!(!is_plain_relative(Path::new("/a")));
        assert!(!is_plain_relative(Path::new("../a")));
        assert!(!is_plain_relative(Path::new("a/../b")));
        assert!(!is_plain_relative(Path::new("./a")));
    }

    #[test]
    fn test_plain_absolute() {
        assert!(is_plain_absolute(Path::new("/")));
        assert!(is_plain_absolute(Path::new("/Users/me/file.txt")));
        assert!(!is_plain_absolute(Path::new("Users/me")));
        assert!(!is_plain_absolute(Path::new("/Users/../etc")));
        assert!(!is_plain_absolute(Path::new("/../etc")));
        assert!(!is_plain_absolute(Path::new("")));
    }

    #[test]
    fn test_ensure_contained_accepts_plain_paths() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("a/b")).unwrap();
        fs::write(root.path().join("a/b/f.txt"), "x").unwrap();

        ensure_contained(root.path(), &root.path().join("a/b/f.txt")).unwrap();
        // Not-yet-existing tails are fine.
        ensure_contained(root.path(), &root.path().join("a/new/deeper/f.txt")).unwrap();
    }

    #[test]
    fn test_ensure_contained_rejects_root_outside_and_dotdot() {
        let root = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();

        for bad in [
            root.path().to_path_buf(),
            other.path().join("f"),
            root.path().join("../escape"),
            root.path().join("a/../../escape"),
        ] {
            let err = ensure_contained(root.path(), &bad).unwrap_err();
            assert!(matches!(err, TuckError::UnsafePath { .. }), "{bad:?}");
        }
    }

    #[test]
    fn test_ensure_contained_rejects_symlink_components() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::create_dir(outside.path().join("data")).unwrap();

        // Symlinked directory in the middle of the path.
        symlink(outside.path(), root.path().join("link")).unwrap();
        let err = ensure_contained(root.path(), &root.path().join("link/data")).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));

        // Symlink as the final component, even a dangling one.
        symlink("/nonexistent/target", root.path().join("dangling")).unwrap();
        let err = ensure_contained(root.path(), &root.path().join("dangling")).unwrap_err();
        assert!(matches!(err, TuckError::UnsafePath { .. }));
    }

    #[test]
    fn test_read_metadata_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("meta.json");
        assert!(read_metadata_file(&file, None).unwrap().is_none());

        fs::write(&file, "hello").unwrap();
        assert_eq!(read_metadata_file(&file, None).unwrap().unwrap(), "hello");
        assert_eq!(read_metadata_file(&file, Some(5)).unwrap().unwrap(), "hello");
        assert!(matches!(
            read_metadata_file(&file, Some(4)).unwrap_err(),
            TuckError::UnsafePath { .. }
        ));

        fs::write(&file, [0xff, 0xfe, 0x00]).unwrap();
        assert!(matches!(
            read_metadata_file(&file, None).unwrap_err(),
            TuckError::UnsafePath { .. }
        ));
    }

    #[test]
    fn test_read_metadata_file_refuses_symlink_and_directory() {
        let dir = TempDir::new().unwrap();
        let secret = dir.path().join("secret");
        fs::write(&secret, "private").unwrap();

        let link = dir.path().join("meta.json");
        symlink(&secret, &link).unwrap();
        assert!(matches!(
            read_metadata_file(&link, None).unwrap_err(),
            TuckError::UnsafePath { .. }
        ));

        let subdir = dir.path().join("dir.json");
        fs::create_dir(&subdir).unwrap();
        assert!(matches!(
            read_metadata_file(&subdir, None).unwrap_err(),
            TuckError::UnsafePath { .. }
        ));
    }

    #[test]
    fn test_write_metadata_file_does_not_follow_symlinks() {
        let dir = TempDir::new().unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, "original").unwrap();
        let target = dir.path().join("meta.json");

        // Symlinks planted at both the temp name and the final name.
        symlink(&victim, tmp_sibling(&target)).unwrap();
        symlink(&victim, &target).unwrap();

        write_metadata_file(&target, b"new data").unwrap();

        assert_eq!(fs::read_to_string(&victim).unwrap(), "original");
        assert!(fs::symlink_metadata(&target).unwrap().file_type().is_file());
        assert_eq!(fs::read_to_string(&target).unwrap(), "new data");
        assert!(fs::symlink_metadata(tmp_sibling(&target)).is_err());
    }

    #[test]
    fn test_write_metadata_file_does_not_create_through_dangling_symlink() {
        let dir = TempDir::new().unwrap();
        let victim = dir.path().join("does-not-exist-yet");
        let target = dir.path().join("meta.json");
        symlink(&victim, &target).unwrap();
        symlink(&victim, tmp_sibling(&target)).unwrap();

        write_metadata_file(&target, b"data").unwrap();

        assert!(fs::symlink_metadata(&victim).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "data");
    }

    #[test]
    fn test_remove_file_if_present_unlinks_symlink_only() {
        let dir = TempDir::new().unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, "keep").unwrap();
        let link = dir.path().join("link");
        symlink(&victim, &link).unwrap();

        remove_file_if_present(&link).unwrap();
        remove_file_if_present(&link).unwrap(); // already gone: still ok

        assert!(fs::symlink_metadata(&link).is_err());
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
    }
}

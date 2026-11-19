use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{TuckError, TuckResult};
use crate::progress::Progress;
use crate::safety;

const GITHUB_API_URL: &str =
    "https://api.github.com/repos/legendword/tuck/releases/latest";
const ASSET_NAME: &str = "tuck-macos-universal";
/// SHA-256 of the binary, published next to it by the release workflow.
const CHECKSUM_ASSET_NAME: &str = "tuck-macos-universal.sha256";
/// Release assets are only ever fetched from this repository's releases.
const DOWNLOAD_URL_PREFIX: &str = "https://github.com/legendword/tuck/releases/download/";
const CHUNK_SIZE: usize = 64 * 1024;

#[derive(Debug)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
    /// Where the release publishes the binary's SHA-256, if it does.
    pub checksum_url: Option<String>,
    pub is_newer: bool,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Parse a version string like "0.1.0" or "v0.1.0" into (major, minor, patch).
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let v = v.strip_prefix('v').unwrap_or(v);
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// An HTTP agent that refuses plain-HTTP URLs, including on redirects.
fn https_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .build()
        .into()
}

/// Check that `url` is `<this repo's release downloads>/<tag>/<asset_name>`.
///
/// The URL comes out of an API response; it is not followed unless it points
/// at the expected asset of this repository.
fn validate_download_url(url: &str, asset_name: &str) -> TuckResult<()> {
    let valid = url
        .strip_prefix(DOWNLOAD_URL_PREFIX)
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(tag, file)| {
            file == asset_name
                && !tag.is_empty()
                && tag != "."
                && tag != ".."
                && tag
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        });
    if valid {
        Ok(())
    } else {
        Err(TuckError::Other(format!(
            "Refusing to download from unexpected URL: {url}"
        )))
    }
}

/// Extract the hash from a `shasum -a 256` style line ("<hex>  <file>").
fn parse_sha256(text: &str) -> Option<String> {
    let token = text.split_whitespace().next()?;
    (token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| token.to_ascii_lowercase())
}

/// Check GitHub for the latest release and compare against the current version.
pub fn check_for_update(current_version: &str) -> TuckResult<UpdateInfo> {
    let response: GitHubRelease = https_agent()
        .get(GITHUB_API_URL)
        .header("User-Agent", "tuck-cli")
        .call()
        .map_err(|e| TuckError::Other(format!("Failed to check for updates: {e}")))?
        .body_mut()
        .read_json()
        .map_err(|e| TuckError::Other(format!("Failed to parse release info: {e}")))?;

    let asset_url = |name: &str| -> TuckResult<Option<String>> {
        match response.assets.iter().find(|a| a.name == name) {
            Some(asset) => {
                validate_download_url(&asset.browser_download_url, name)?;
                Ok(Some(asset.browser_download_url.clone()))
            }
            None => Ok(None),
        }
    };
    let download_url = asset_url(ASSET_NAME)?
        .ok_or_else(|| TuckError::Other("No compatible binary found in latest release".into()))?;
    let checksum_url = asset_url(CHECKSUM_ASSET_NAME)?;

    let latest = response.tag_name.strip_prefix('v').unwrap_or(&response.tag_name);
    let is_newer = match (parse_version(current_version), parse_version(latest)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    };

    Ok(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: latest.to_string(),
        download_url,
        checksum_url,
        is_newer,
    })
}

/// Download the latest binary, verify its checksum, and replace the current executable.
pub fn execute_update(
    info: &UpdateInfo,
    progress: Option<&dyn Progress>,
) -> TuckResult<()> {
    let checksum_url = info.checksum_url.as_deref().ok_or_else(|| {
        TuckError::Other(
            "The latest release does not publish a checksum for the binary; \
             refusing to install it unverified."
                .into(),
        )
    })?;
    validate_download_url(&info.download_url, ASSET_NAME)?;
    validate_download_url(checksum_url, CHECKSUM_ASSET_NAME)?;

    let current_exe = std::env::current_exe()
        .map_err(|e| TuckError::Other(format!("Cannot determine current executable path: {e}")))?
        .canonicalize()
        .map_err(|e| TuckError::Other(format!("Cannot resolve executable path: {e}")))?;

    let parent = current_exe
        .parent()
        .ok_or_else(|| TuckError::Other("Cannot determine executable directory".into()))?;
    let tmp_path = parent.join(".tuck-update.tmp");

    let agent = https_agent();

    let checksum_text = agent
        .get(checksum_url)
        .header("User-Agent", "tuck-cli")
        .call()
        .map_err(|e| TuckError::Other(format!("Failed to download checksum: {e}")))?
        .body_mut()
        .read_to_string()
        .map_err(|e| TuckError::Other(format!("Failed to read checksum: {e}")))?;
    let expected_sha256 = parse_sha256(&checksum_text)
        .ok_or_else(|| TuckError::Other("The release checksum file is malformed".into()))?;

    // Download the new binary
    let mut response = agent
        .get(&info.download_url)
        .header("User-Agent", "tuck-cli")
        .call()
        .map_err(|e| TuckError::Other(format!("Failed to download update: {e}")))?;

    let content_length = response.body().content_length().unwrap_or(0);

    if let Some(p) = progress {
        p.start_phase("Downloading", content_length);
    }
    let mut reader = response.body_mut().as_reader();
    let downloaded = download_verified(&mut reader, &tmp_path, &expected_sha256, progress);
    if let Some(p) = progress {
        p.finish_phase();
    }
    downloaded?;

    // Remove macOS quarantine attribute
    let _ = std::process::Command::new("xattr")
        .args(["-d", "com.apple.quarantine"])
        .arg(&tmp_path)
        .output();

    // Atomic rename over the current binary
    fs::rename(&tmp_path, &current_exe).map_err(|e| {
        // Clean up temp file on failure
        let _ = fs::remove_file(&tmp_path);
        if e.kind() == ErrorKind::PermissionDenied {
            permission_denied()
        } else {
            TuckError::Other(format!("Failed to replace binary: {e}"))
        }
    })?;

    Ok(())
}

fn permission_denied() -> TuckError {
    TuckError::Other("Permission denied. Try running with sudo: sudo tuck update".into())
}

/// Stream `reader` into a new file at `tmp_path` and check its SHA-256.
///
/// The file is created exclusively, so a file or symlink already sitting at
/// that name is replaced rather than written through. It only becomes
/// executable once the checksum has matched, and it is removed again if
/// anything fails.
fn download_verified(
    reader: &mut dyn Read,
    tmp_path: &Path,
    expected_sha256: &str,
    progress: Option<&dyn Progress>,
) -> TuckResult<()> {
    let create_error = |e: std::io::Error| {
        if e.kind() == ErrorKind::PermissionDenied {
            permission_denied()
        } else {
            TuckError::Other(format!("Cannot create temp file: {e}"))
        }
    };
    safety::remove_file_if_present(tmp_path).map_err(|e| match e {
        TuckError::Io { source, .. } => create_error(source),
        other => other,
    })?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(tmp_path)
        .map_err(create_error)?;

    let mut write_and_verify = || -> TuckResult<()> {
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; CHUNK_SIZE];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| TuckError::Other(format!("Download interrupted: {e}")))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(|e| TuckError::Other(format!("Failed to write temp file: {e}")))?;
            if let Some(p) = progress {
                p.advance(n as u64);
            }
        }
        file.sync_all()
            .map_err(|e| TuckError::Other(format!("Failed to write temp file: {e}")))?;

        let actual = format!("{:x}", hasher.finalize());
        if actual != expected_sha256 {
            return Err(TuckError::ChecksumMismatch {
                path: tmp_path.to_path_buf(),
                expected: expected_sha256.to_string(),
                actual,
            });
        }

        // Set executable permissions
        fs::set_permissions(tmp_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| TuckError::Other(format!("Failed to set permissions: {e}")))
    };

    let result = write_and_verify();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(tmp_path);
    }
    result
}

/// Return the path of the currently running executable.
pub fn current_exe_path() -> TuckResult<PathBuf> {
    std::env::current_exe()
        .map_err(|e| TuckError::Other(format!("Cannot determine executable path: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    const PAYLOAD: &[u8] = b"new tuck binary";

    fn sha256_hex(data: &[u8]) -> String {
        format!("{:x}", Sha256::digest(data))
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("10.20.30"), Some((10, 20, 30)));
        assert_eq!(parse_version("bad"), None);
        assert_eq!(parse_version("1.2"), None);
    }

    #[test]
    fn test_version_comparison() {
        let v = |s| parse_version(s).unwrap();
        assert!(v("0.2.0") > v("0.1.0"));
        assert!(v("1.0.0") > v("0.9.9"));
        assert!(v("0.1.1") > v("0.1.0"));
        assert!(!(v("0.1.0") > v("0.1.0")));
    }

    #[test]
    fn test_validate_download_url() {
        let ok = "https://github.com/legendword/tuck/releases/download/v0.3.0/tuck-macos-universal";
        validate_download_url(ok, ASSET_NAME).unwrap();
        validate_download_url(&format!("{ok}.sha256"), CHECKSUM_ASSET_NAME).unwrap();

        for bad in [
            "http://github.com/legendword/tuck/releases/download/v0.3.0/tuck-macos-universal",
            "https://evil.example/legendword/tuck/releases/download/v0.3.0/tuck-macos-universal",
            "https://github.com/someone-else/tuck/releases/download/v0.3.0/tuck-macos-universal",
            "https://github.com.evil.example/legendword/tuck/releases/download/v1/tuck-macos-universal",
            "https://github.com/legendword/tuck/releases/download/../../../../evil/x/releases/download/v1/tuck-macos-universal",
            "https://github.com/legendword/tuck/releases/download/../tuck-macos-universal",
            "https://github.com/legendword/tuck/releases/download/v0.3.0/other-file",
            "https://github.com/legendword/tuck/releases/download/v0.3.0/tuck-macos-universal?x=1",
            "https://github.com/legendword/tuck/releases/download/tuck-macos-universal",
            "",
        ] {
            assert!(validate_download_url(bad, ASSET_NAME).is_err(), "{bad}");
        }
    }

    #[test]
    fn test_parse_sha256() {
        let hex = sha256_hex(PAYLOAD);
        assert_eq!(parse_sha256(&format!("{hex}  tuck-macos-universal\n")), Some(hex.clone()));
        assert_eq!(parse_sha256(&hex.to_uppercase()), Some(hex.clone()));
        assert_eq!(parse_sha256(""), None);
        assert_eq!(parse_sha256("not-a-hash  file"), None);
        assert_eq!(parse_sha256(&hex[..63]), None);
        assert_eq!(parse_sha256("<html>404 Not Found</html>"), None);
    }

    #[test]
    fn test_download_verified_accepts_matching_checksum() {
        let dir = TempDir::new().unwrap();
        let tmp = dir.path().join(".tuck-update.tmp");

        download_verified(&mut &PAYLOAD[..], &tmp, &sha256_hex(PAYLOAD), None).unwrap();

        assert_eq!(fs::read(&tmp).unwrap(), PAYLOAD);
        let mode = fs::metadata(&tmp).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn test_download_verified_rejects_wrong_checksum() {
        let dir = TempDir::new().unwrap();
        let tmp = dir.path().join(".tuck-update.tmp");

        let err = download_verified(
            &mut &b"tampered binary"[..],
            &tmp,
            &sha256_hex(PAYLOAD),
            None,
        )
        .unwrap_err();

        assert!(matches!(err, TuckError::ChecksumMismatch { .. }));
        assert!(fs::symlink_metadata(&tmp).is_err(), "temp file must be removed");
    }

    #[test]
    fn test_download_verified_does_not_write_through_symlink() {
        let dir = TempDir::new().unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, "original").unwrap();
        let tmp = dir.path().join(".tuck-update.tmp");
        symlink(&victim, &tmp).unwrap();

        download_verified(&mut &PAYLOAD[..], &tmp, &sha256_hex(PAYLOAD), None).unwrap();

        assert_eq!(fs::read_to_string(&victim).unwrap(), "original");
        assert!(fs::symlink_metadata(&tmp).unwrap().file_type().is_file());
        assert_eq!(fs::read(&tmp).unwrap(), PAYLOAD);
    }
}

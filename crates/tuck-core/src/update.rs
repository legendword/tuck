use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde::Deserialize;

use crate::error::{TuckError, TuckResult};
use crate::progress::Progress;

const GITHUB_API_URL: &str =
    "https://api.github.com/repos/legendword/tuck/releases/latest";
const ASSET_NAME: &str = "tuck-macos-universal";
const CHUNK_SIZE: usize = 64 * 1024;

#[derive(Debug)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
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

/// Check GitHub for the latest release and compare against the current version.
pub fn check_for_update(current_version: &str) -> TuckResult<UpdateInfo> {
    let response: GitHubRelease = ureq::get(GITHUB_API_URL)
        .header("User-Agent", "tuck-cli")
        .call()
        .map_err(|e| TuckError::Other(format!("Failed to check for updates: {e}")))?
        .body_mut()
        .read_json()
        .map_err(|e| TuckError::Other(format!("Failed to parse release info: {e}")))?;

    let asset = response
        .assets
        .iter()
        .find(|a| a.name == ASSET_NAME)
        .ok_or_else(|| TuckError::Other("No compatible binary found in latest release".into()))?;

    let latest = response.tag_name.strip_prefix('v').unwrap_or(&response.tag_name);
    let is_newer = match (parse_version(current_version), parse_version(latest)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    };

    Ok(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: latest.to_string(),
        download_url: asset.browser_download_url.clone(),
        is_newer,
    })
}

/// Download the latest binary and replace the current executable.
pub fn execute_update(
    info: &UpdateInfo,
    progress: Option<&dyn Progress>,
) -> TuckResult<()> {
    let current_exe = std::env::current_exe()
        .map_err(|e| TuckError::Other(format!("Cannot determine current executable path: {e}")))?
        .canonicalize()
        .map_err(|e| TuckError::Other(format!("Cannot resolve executable path: {e}")))?;

    let parent = current_exe
        .parent()
        .ok_or_else(|| TuckError::Other("Cannot determine executable directory".into()))?;
    let tmp_path = parent.join(".tuck-update.tmp");

    // Download the new binary
    let mut response = ureq::get(&info.download_url)
        .header("User-Agent", "tuck-cli")
        .call()
        .map_err(|e| TuckError::Other(format!("Failed to download update: {e}")))?;

    let content_length = response.body().content_length().unwrap_or(0);

    if let Some(p) = progress {
        p.start_phase("Downloading", content_length);
    }

    let mut reader = response.body_mut().as_reader();
    let mut file = fs::File::create(&tmp_path)
        .map_err(|e| TuckError::Other(format!("Cannot create temp file: {e}")))?;

    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| TuckError::Other(format!("Download interrupted: {e}")))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .map_err(|e| TuckError::Other(format!("Failed to write temp file: {e}")))?;
        if let Some(p) = progress {
            p.advance(n as u64);
        }
    }
    drop(file);

    if let Some(p) = progress {
        p.finish_phase();
    }

    // Set executable permissions
    fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755))
        .map_err(|e| TuckError::Other(format!("Failed to set permissions: {e}")))?;

    // Remove macOS quarantine attribute
    let _ = std::process::Command::new("xattr")
        .args(["-d", "com.apple.quarantine"])
        .arg(&tmp_path)
        .output();

    // Atomic rename over the current binary
    fs::rename(&tmp_path, &current_exe).map_err(|e| {
        // Clean up temp file on failure
        let _ = fs::remove_file(&tmp_path);
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            TuckError::Other(
                "Permission denied. Try running with sudo: sudo tuck update".into(),
            )
        } else {
            TuckError::Other(format!("Failed to replace binary: {e}"))
        }
    })?;

    Ok(())
}

/// Return the path of the currently running executable.
pub fn current_exe_path() -> TuckResult<PathBuf> {
    std::env::current_exe()
        .map_err(|e| TuckError::Other(format!("Cannot determine executable path: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

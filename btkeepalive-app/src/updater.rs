//! Update check, verify, and install.
//!
//! The flow hits the GitHub releases API, picks `BTKeepAlive.exe` plus
//! `SHA256SUMS.txt`, verifies the hash, then hot swaps. Pure logic here
//! is platform-free and unit tested; the process swap only runs on Windows.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Default update source repo.
pub const DEFAULT_REPO: &str = "kadato/bt-keepalive";
/// Expected asset names on the release.
pub const EXE_ASSET: &str = "BTKeepAlive.exe";
pub const CHECKSUM_ASSET: &str = "SHA256SUMS.txt";

/// A newer release with download locations.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateInfo {
    /// Release tag, for example `v2.1.0`.
    pub version: String,
    /// Release notes body, may be empty.
    pub notes: String,
    /// Direct download URL for the exe.
    pub download_url: String,
    /// Direct download URL for the checksum file.
    pub checksum_url: String,
}

/// Parse `v2.1.0` or `2.2.0-beta` into comparable integers.
#[must_use]
pub fn parse_version(s: &str) -> Vec<u64> {
    s.trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .split('.')
        .filter_map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                None
            } else {
                digits.parse().ok()
            }
        })
        .collect()
}

/// True when `latest` is newer than `current`.
#[must_use]
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse_version(latest), parse_version(current));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

/// Pick exe plus checksum URLs from a GitHub release JSON payload.
#[must_use]
pub fn pick_assets(release: &serde_json::Value) -> Option<(String, String)> {
    let assets = release.get("assets")?.as_array()?;
    let mut exe = None;
    let mut sums = None;
    for asset in assets {
        let name = asset.get("name")?.as_str()?;
        let url = asset.get("browser_download_url")?.as_str()?;
        if name == EXE_ASSET {
            exe = Some(url.to_string());
        } else if name == CHECKSUM_ASSET {
            sums = Some(url.to_string());
        }
    }
    Some((exe?, sums?))
}

/// Fetch the latest release and return update info when newer.
pub fn check_for_update(repo: &str, current: &str) -> Result<Option<UpdateInfo>, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let release: serde_json::Value = ureq::get(&url)
        .set("User-Agent", "BTKeepAlive-Updater")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("release query failed: {e}"))?
        .into_json()
        .map_err(|e| format!("release parse failed: {e}"))?;
    let tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if tag.is_empty() || !is_newer(tag, current) {
        return Ok(None);
    }
    let notes = release
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    match pick_assets(&release) {
        Some((download_url, checksum_url)) => Ok(Some(UpdateInfo {
            version: tag.to_string(),
            notes,
            download_url,
            checksum_url,
        })),
        None => Err("release is missing exe or checksum assets".to_string()),
    }
}

/// Download the expected SHA256 for the exe from the checksum file.
pub fn fetch_expected_sha256(url: &str) -> Result<String, String> {
    let text = ureq::get(url)
        .set("User-Agent", "BTKeepAlive-Updater")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("checksum fetch failed: {e}"))?
        .into_string()
        .map_err(|e| format!("checksum read failed: {e}"))?;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(hash), Some(name)) = (parts.next(), parts.next()) {
            if name == EXE_ASSET {
                return Ok(hash.to_lowercase());
            }
        }
    }
    Err("checksum file has no exe entry".to_string())
}

/// SHA256 hex of a file.
pub fn file_sha256(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Download with progress reports and cooperative cancellation.
pub fn download_file(
    url: &str,
    dest: &Path,
    progress: impl Fn(u64, u64),
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let response = ureq::get(url)
        .set("User-Agent", "BTKeepAlive-Updater")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let total: u64 = response
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut reader = response.into_reader();
    let mut out = File::create(dest).map_err(|e| format!("cannot write file: {e}"))?;
    let mut downloaded = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(dest);
            return Err("cancelled".to_string());
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        io::Write::write_all(&mut out, &buf[..n]).map_err(|e| format!("cannot write file: {e}"))?;
        downloaded += n as u64;
        progress(downloaded, total);
    }
    Ok(())
}

/// Verify, then swap the running exe on Windows.
///
/// Non-Windows builds only download plus verify next to `current_exe`,
/// which keeps the whole flow testable without touching a live binary.
pub fn install_update(
    info: &UpdateInfo,
    current_exe: &Path,
    progress: impl Fn(u64, u64),
    cancelled: &AtomicBool,
) -> Result<PathBuf, String> {
    let expected = fetch_expected_sha256(&info.checksum_url)?;
    let new_path = current_exe.with_extension("exe.new");
    download_file(&info.download_url, &new_path, progress, cancelled)?;
    let actual = file_sha256(&new_path).map_err(|e| format!("checksum read failed: {e}"))?;
    if actual != expected {
        let _ = std::fs::remove_file(&new_path);
        return Err("SHA256 mismatch; the file may be corrupted".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        hot_swap_windows(current_exe, &new_path)?;
    }
    Ok(new_path)
}

/// Rename running exe aside, move the new one in, relaunch detached.
#[cfg(target_os = "windows")]
fn hot_swap_windows(current_exe: &Path, new_path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;

    let old_path = current_exe.with_extension("exe.old");
    let _ = std::fs::remove_file(&old_path);
    std::fs::rename(current_exe, &old_path).map_err(|e| format!("cannot stage old exe: {e}"))?;
    if let Err(e) = std::fs::rename(new_path, current_exe) {
        let _ = std::fs::rename(&old_path, current_exe);
        return Err(format!("cannot place new exe: {e}"));
    }
    std::process::Command::new(current_exe)
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map_err(|e| format!("cannot relaunch: {e}"))?;
    std::process::exit(0);
}

/// Remove a stale `.exe.old` left by a previous update.
pub fn cleanup_old_version(current_exe: &Path) {
    let old = current_exe.with_extension("exe.old");
    if old.is_file() {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse_handles_tags() {
        assert_eq!(parse_version("v2.1.0"), vec![2, 1, 0]);
        assert_eq!(parse_version("2.2.0-beta"), vec![2, 2, 0]);
        assert!(is_newer("v2.1.0", "2.0.9"));
        assert!(!is_newer("v2.0.0", "v2.0.0"));
        assert!(!is_newer("v2.0.0", "v2.1.0"));
        assert!(is_newer("v2.0.1", "v2.0"));
    }

    #[test]
    fn picks_assets_from_release_json() {
        let release = serde_json::json!({
            "tag_name": "v2.1.0",
            "assets": [
                {"name": "BTKeepAlive.exe", "browser_download_url": "https://x/e"},
                {"name": "SHA256SUMS.txt", "browser_download_url": "https://x/s"},
            ],
        });
        assert_eq!(
            pick_assets(&release),
            Some(("https://x/e".to_string(), "https://x/s".to_string()))
        );
    }

    #[test]
    fn rejects_release_missing_assets() {
        let release = serde_json::json!({"tag_name": "v2.1.0", "assets": []});
        assert_eq!(pick_assets(&release), None);
    }

    #[test]
    fn sha256_of_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            file_sha256(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

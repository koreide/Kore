use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Default for [`crate::CoreConfig::update_url`].
pub const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/koreide/Kore/releases/latest";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub has_update: bool,
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub release_notes: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// In-app install (download DMG, replace /Applications/kore.app) exists only on macOS.
/// Elsewhere the UI sends the user to the release page instead.
pub const SELF_UPDATE_SUPPORTED: bool = cfg!(target_os = "macos");

/// Strip a leading 'v' from a version string for comparison.
fn normalize_version(v: &str) -> &str {
    v.strip_prefix('v').unwrap_or(v)
}

/// Numeric `major.minor.patch` parts; anything after `-`/`+` (pre-release, build) is ignored.
fn version_parts(v: &str) -> Option<Vec<u64>> {
    let core = normalize_version(v.trim()).split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// True when `latest` is a strictly newer version than `current`. Unparsable versions
/// fall back to "differs", so a malformed tag still surfaces rather than hiding updates.
fn is_newer(latest: &str, current: &str) -> bool {
    match (version_parts(latest), version_parts(current)) {
        (Some(mut l), Some(mut c)) => {
            let n = l.len().max(c.len());
            l.resize(n, 0);
            c.resize(n, 0);
            l > c
        }
        _ => normalize_version(latest) != normalize_version(current),
    }
}

pub async fn check_for_updates(releases_url: &str) -> Result<UpdateInfo, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();

    let client = reqwest::Client::builder()
        .user_agent("Kore-Update-Checker")
        .build()
        .map_err(|e| e.to_string())?;

    let release: GitHubRelease = client
        .get(releases_url)
        .send()
        .await
        .map_err(|e| {
            warn!(error = %e, "Failed to check for updates");
            format!("Failed to reach GitHub: {e}")
        })?
        .json()
        .await
        .map_err(|e| {
            warn!(error = %e, "Failed to parse release response");
            format!("Failed to parse release info: {e}")
        })?;

    let has_update = is_newer(&release.tag_name, &current_version);

    Ok(UpdateInfo {
        has_update,
        current_version,
        latest_version: release.tag_name,
        release_url: release.html_url,
        release_notes: release.body.unwrap_or_default(),
    })
}

/// Find the DMG for `arch` (`std::env::consts::ARCH`). Only an exact architecture match
/// is accepted: installing the other architecture's app would not run (or run under Rosetta).
/// Intel DMGs are named `x86_64`; older releases used `x64`.
fn find_dmg_url<'a>(assets: &'a [GitHubAsset], arch: &str) -> Option<&'a str> {
    let labels: &[&str] = match arch {
        "aarch64" => &["aarch64"],
        "x86_64" => &["x86_64", "x64"],
        _ => return None,
    };
    assets
        .iter()
        .find(|a| {
            let name = a.name.to_lowercase();
            name.ends_with(".dmg") && labels.iter().any(|l| name.contains(&format!("_{l}")))
        })
        .map(|a| a.browser_download_url.as_str())
}

/// Download the latest DMG, mount it, copy .app to /Applications, unmount (macOS only).
pub async fn perform_update(releases_url: &str) -> Result<String, String> {
    if !SELF_UPDATE_SUPPORTED {
        return Err("In-app update is only available on macOS; download the new version from the release page".into());
    }
    let client = reqwest::Client::builder()
        .user_agent("Kore-Update-Checker")
        .build()
        .map_err(|e| e.to_string())?;

    info!("Fetching latest release info for update...");
    let release: GitHubRelease = client
        .get(releases_url)
        .send()
        .await
        .map_err(|e| format!("Failed to reach GitHub: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {e}"))?;

    let dmg_url = find_dmg_url(&release.assets, std::env::consts::ARCH)
        .ok_or_else(|| "No DMG asset found for your architecture".to_string())?;

    info!(url = %dmg_url, "Downloading DMG...");
    let dmg_bytes = client
        .get(dmg_url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {e}"))?;

    // Writing the DMG, hdiutil, rm/cp and xattr are blocking: run them on the
    // blocking pool, not on an async worker thread (A14).
    let dest_dir = std::path::PathBuf::from("/Applications");
    tokio::task::spawn_blocking(move || install_dmg(&dmg_bytes, &dest_dir))
        .await
        .map_err(|e| format!("Install task failed: {e}"))??;

    info!(version = %release.tag_name, "Update installed successfully");
    Ok(release.tag_name)
}

/// Detaches the mounted DMG when dropped (every exit path, including errors).
struct Mounted<'a>(&'a std::path::Path);

impl Drop for Mounted<'_> {
    fn drop(&mut self) {
        let _ = std::process::Command::new("hdiutil")
            .arg("detach")
            .arg(self.0)
            .arg("-quiet")
            .output();
    }
}

/// Blocking: write `dmg_bytes` to a temp file, mount it, replace the `.app`
/// in `dest_dir` with the one in the image, unmount and clear quarantine.
/// Returns the installed bundle path.
fn install_dmg(dmg_bytes: &[u8], dest_dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let tmp_dir = tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {e}"))?;
    let dmg_path = tmp_dir.path().join("Kore-update.dmg");
    std::fs::write(&dmg_path, dmg_bytes).map_err(|e| format!("Failed to write DMG: {e}"))?;

    info!("Mounting DMG...");
    let mount_point = tmp_dir.path().join("kore_mount");
    std::fs::create_dir_all(&mount_point)
        .map_err(|e| format!("Failed to create mount point: {e}"))?;

    let mount_out = std::process::Command::new("hdiutil")
        .arg("attach")
        .arg(&dmg_path)
        .arg("-nobrowse")
        .arg("-quiet")
        .arg("-mountpoint")
        .arg(&mount_point)
        .output()
        .map_err(|e| format!("Failed to run hdiutil attach: {e}"))?;

    if !mount_out.status.success() {
        return Err(format!(
            "Failed to mount DMG: {}",
            String::from_utf8_lossy(&mount_out.stderr)
        ));
    }
    let mounted = Mounted(&mount_point);

    let app_path = find_app_bundle(&mount_point)?;
    let app_name = app_path
        .file_name()
        .ok_or_else(|| "DMG app bundle has no file name".to_string())?
        .to_string_lossy()
        .to_string();
    let dest = dest_dir.join(&app_name);

    info!(dest = %dest.display(), "Installing...");

    // Remove existing
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|e| format!("Failed to remove old installation: {e}"))?;
    }

    // Copy new .app
    let cp_out = std::process::Command::new("cp")
        .arg("-R")
        .arg(&app_path)
        .arg(&dest)
        .output()
        .map_err(|e| format!("Failed to copy app: {e}"))?;

    if !cp_out.status.success() {
        return Err(format!(
            "Failed to copy to {}: {}",
            dest_dir.display(),
            String::from_utf8_lossy(&cp_out.stderr)
        ));
    }

    // Unmount
    drop(mounted);

    // Clear quarantine
    let _ = std::process::Command::new("xattr")
        .arg("-dr")
        .arg("com.apple.quarantine")
        .arg(&dest)
        .output();

    Ok(dest)
}

/// The first `*.app` entry in `dir`.
fn find_app_bundle(dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read mount point: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().map(|ext| ext == "app").unwrap_or(false))
        .ok_or_else(|| "No .app found in DMG".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GitHubAsset {
        GitHubAsset { name: name.into(), browser_download_url: format!("https://dl/{name}") }
    }

    #[test]
    fn newer_versions_only() {
        assert!(is_newer("v0.1.3", "0.1.2"));
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("v1.0", "0.9.9"));
        assert!(!is_newer("v0.1.2", "0.1.2"));
        assert!(!is_newer("v0.1.2", "0.1.10"), "an older release must not be offered as an update");
        assert!(!is_newer("v0.1.2-rc1", "0.1.2"));
        assert!(is_newer("nightly", "0.1.2"), "unparsable tags fall back to 'differs'");
    }

    #[test]
    fn dmg_matches_architecture_only() {
        let assets = vec![
            asset("kore_0.2.0_aarch64.dmg"),
            asset("kore_0.2.0_x86_64.dmg"),
            asset("kore_0.2.0_x86_64.tar.gz"),
            asset("kore_0.2.0_x64-setup.exe"),
        ];
        assert_eq!(find_dmg_url(&assets, "aarch64"), Some("https://dl/kore_0.2.0_aarch64.dmg"));
        assert_eq!(find_dmg_url(&assets, "x86_64"), Some("https://dl/kore_0.2.0_x86_64.dmg"));
        // Older releases named the Intel DMG `x64`; never fall back to the other architecture.
        let old = vec![asset("kore_0.1.2_aarch64.dmg"), asset("kore_0.1.2_x64.dmg")];
        assert_eq!(find_dmg_url(&old, "x86_64"), Some("https://dl/kore_0.1.2_x64.dmg"));
        assert_eq!(find_dmg_url(&old[..1], "x86_64"), None);
        assert_eq!(find_dmg_url(&assets, "riscv64"), None);
    }

    #[test]
    fn app_bundle_lookup() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_app_bundle(dir.path()).unwrap_err(), "No .app found in DMG");
        std::fs::write(dir.path().join("README.txt"), "x").unwrap();
        std::fs::create_dir(dir.path().join("kore.app")).unwrap();
        assert_eq!(find_app_bundle(dir.path()).unwrap(), dir.path().join("kore.app"));
        assert!(find_app_bundle(&dir.path().join("missing")).is_err());
    }

    /// The install step is plain blocking code (A14) and fails cleanly on a
    /// bad image without touching the destination.
    #[cfg(target_os = "macos")]
    #[test]
    fn install_rejects_a_non_dmg_without_touching_dest() {
        let dest = tempfile::tempdir().unwrap();
        let err = install_dmg(b"definitely not a disk image", dest.path()).unwrap_err();
        assert!(err.starts_with("Failed to mount DMG"), "{err}");
        assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
    }
}

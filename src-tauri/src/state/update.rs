use serde::{Deserialize, Serialize};
use tracing::{info, warn};

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/koreide/Kore/releases/latest";

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

/// Strip a leading 'v' from a version string for comparison.
fn normalize_version(v: &str) -> &str {
    v.strip_prefix('v').unwrap_or(v)
}

pub async fn check_for_updates() -> Result<UpdateInfo, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();

    let client = reqwest::Client::builder()
        .user_agent("Kore-Update-Checker")
        .build()
        .map_err(|e| e.to_string())?;

    let release: GitHubRelease = client
        .get(GITHUB_RELEASES_URL)
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

    let latest = normalize_version(&release.tag_name);
    let current = normalize_version(&current_version);
    let has_update = latest != current;

    Ok(UpdateInfo {
        has_update,
        current_version,
        latest_version: release.tag_name,
        release_url: release.html_url,
        release_notes: release.body.unwrap_or_default(),
    })
}

/// Find the DMG download URL for the current architecture.
fn find_dmg_url(assets: &[GitHubAsset]) -> Option<&str> {
    let arch_label = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        _ => return None,
    };

    // Try arch-specific DMG first
    let url = assets
        .iter()
        .find(|a| {
            let name = a.name.to_lowercase();
            name.ends_with(".dmg") && name.contains(arch_label)
        })
        .or_else(|| {
            // Fallback: any DMG
            assets.iter().find(|a| a.name.to_lowercase().ends_with(".dmg"))
        })
        .map(|a| a.browser_download_url.as_str());

    url
}

/// Download the latest DMG, mount it, copy .app to /Applications, unmount.
pub async fn perform_update() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("Kore-Update-Checker")
        .build()
        .map_err(|e| e.to_string())?;

    info!("Fetching latest release info for update...");
    let release: GitHubRelease = client
        .get(GITHUB_RELEASES_URL)
        .send()
        .await
        .map_err(|e| format!("Failed to reach GitHub: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {e}"))?;

    let dmg_url = find_dmg_url(&release.assets)
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

    // Write to temp file
    let tmp_dir = tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {e}"))?;
    let dmg_path = tmp_dir.path().join("Kore-update.dmg");
    std::fs::write(&dmg_path, &dmg_bytes)
        .map_err(|e| format!("Failed to write DMG: {e}"))?;

    info!("Mounting DMG...");
    let mount_point = tmp_dir.path().join("kore_mount");
    std::fs::create_dir_all(&mount_point)
        .map_err(|e| format!("Failed to create mount point: {e}"))?;

    let mount_out = std::process::Command::new("hdiutil")
        .args([
            "attach",
            dmg_path.to_str().unwrap(),
            "-nobrowse",
            "-quiet",
            "-mountpoint",
            mount_point.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| format!("Failed to run hdiutil attach: {e}"))?;

    if !mount_out.status.success() {
        return Err(format!(
            "Failed to mount DMG: {}",
            String::from_utf8_lossy(&mount_out.stderr)
        ));
    }

    // Find .app in mounted volume
    let app_entry = std::fs::read_dir(&mount_point)
        .map_err(|e| format!("Failed to read mount point: {e}"))?
        .filter_map(|e| e.ok())
        .find(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "app")
                .unwrap_or(false)
        });

    let app_path = match app_entry {
        Some(entry) => entry.path(),
        None => {
            let _ = std::process::Command::new("hdiutil")
                .args(["detach", mount_point.to_str().unwrap(), "-quiet"])
                .output();
            return Err("No .app found in DMG".to_string());
        }
    };

    let app_name = app_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let dest = std::path::PathBuf::from("/Applications").join(&app_name);

    info!(dest = %dest.display(), "Installing...");

    // Remove existing
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|e| format!("Failed to remove old installation: {e}"))?;
    }

    // Copy new .app
    let cp_out = std::process::Command::new("cp")
        .args(["-R", app_path.to_str().unwrap(), dest.to_str().unwrap()])
        .output()
        .map_err(|e| format!("Failed to copy app: {e}"))?;

    if !cp_out.status.success() {
        let _ = std::process::Command::new("hdiutil")
            .args(["detach", mount_point.to_str().unwrap(), "-quiet"])
            .output();
        return Err(format!(
            "Failed to copy to /Applications: {}",
            String::from_utf8_lossy(&cp_out.stderr)
        ));
    }

    // Unmount
    let _ = std::process::Command::new("hdiutil")
        .args(["detach", mount_point.to_str().unwrap(), "-quiet"])
        .output();

    // Clear quarantine
    let _ = std::process::Command::new("xattr")
        .args(["-dr", "com.apple.quarantine", dest.to_str().unwrap()])
        .output();

    info!(version = %release.tag_name, "Update installed successfully");
    Ok(release.tag_name)
}

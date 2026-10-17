//! Cross-platform subprocess resolution (`kubectl`, `helm`, `claude`, `agent`)
//! and PATH enrichment for GUI launches (Finder/Dock/Start menu provide a
//! minimal PATH).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Extra directories appended to PATH (if they exist) on Unix-like systems.
fn extra_unix_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/sbin",
        "/usr/local/aws-cli",
        "/Library/Frameworks/Python.framework/Versions/Current/bin",
        "/snap/bin",
        "/nix/var/nix/profiles/default/bin",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = dirs::home_dir() {
        for rel in [
            ".local/bin",
            "bin",
            ".krew/bin",
            "google-cloud-sdk/bin",
            ".nix-profile/bin",
            ".npm-global/bin",
            ".rd/bin",
            ".docker/bin",
        ] {
            dirs.push(home.join(rel));
        }
    }
    dirs
}

/// Extra directories appended to PATH on Windows.
fn extra_windows_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("npm"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        dirs.push(local.join("Microsoft").join("WinGet").join("Links"));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let profile = PathBuf::from(profile);
        dirs.push(profile.join("scoop").join("shims"));
        dirs.push(profile.join(".krew").join("bin"));
    }
    if let Some(pd) = std::env::var_os("ProgramData") {
        dirs.push(PathBuf::from(pd).join("chocolatey").join("bin"));
    }
    dirs
}

fn extra_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        extra_windows_dirs()
    } else {
        extra_unix_dirs()
    }
}

/// Compute an enriched PATH value: the current PATH followed by any existing
/// well-known tool directories that are not already present. Uses the
/// platform separator (`:` on Unix, `;` on Windows).
pub fn enriched_path() -> OsString {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::env::split_paths(&current).collect();
    for dir in extra_dirs() {
        if !paths.iter().any(|p| p == &dir) && dir.is_dir() {
            paths.push(dir);
        }
    }
    std::env::join_paths(paths).unwrap_or(current)
}

/// Enrich the process PATH so exec-credential plugins (aws, gcloud, az,
/// kubelogin), `kubectl`, `helm`, `claude` and `agent` resolve when the app
/// is launched from a GUI shell.
///
/// Call this at the very start of `main()`, before any threads are spawned
/// (it mutates the process environment).
pub fn enrich_path() {
    let value = enriched_path();
    std::env::set_var("PATH", value);
}

/// Executable extensions to try for a bare program name.
fn candidate_names(name: &str) -> Vec<String> {
    if cfg!(windows) && Path::new(name).extension().is_none() {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut out: Vec<String> = pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| format!("{name}{}", e.to_ascii_lowercase()))
            .collect();
        // Make sure the common ones are present even with an odd PATHEXT.
        for ext in [".exe", ".cmd", ".bat"] {
            let n = format!("{name}{ext}");
            if !out.contains(&n) {
                out.push(n);
            }
        }
        out
    } else {
        vec![name.to_string()]
    }
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// [`crate::CoreConfig::tool_dir`] of the Core running this thread.
fn tool_dir() -> Option<PathBuf> {
    crate::ambient::get().and_then(|c| c.tool_dir.clone())
}

/// Look up `name` on PATH plus the well-known tool directories.
pub fn which(name: &str) -> Option<PathBuf> {
    if let Some(dir) = tool_dir() {
        let candidate = dir.join(name);
        return is_executable(&candidate).then_some(candidate);
    }
    let path = enriched_path();
    let names = candidate_names(name);
    for dir in std::env::split_paths(&path) {
        for n in &names {
            let candidate = dir.join(n);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Resolve the program to run for a tool. Honours an override env var
/// (`KORE_KUBECTL`, `KORE_HELM`, `KORE_CLAUDE`, `KORE_AGENT`), then PATH
/// lookup (with `.exe`/`.cmd` on Windows). Falls back to the bare name so the
/// spawn error message stays meaningful.
pub fn resolve(name: &str) -> PathBuf {
    if let Some(dir) = tool_dir() {
        return dir.join(name);
    }
    let env_key = format!("KORE_{}", name.to_ascii_uppercase());
    if let Some(p) = std::env::var_os(&env_key) {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    which(name).unwrap_or_else(|| PathBuf::from(name))
}

/// A `tokio::process::Command` for the resolved tool, with PATH enriched for
/// the child (so credential plugins it launches resolve too) and
/// `kill_on_drop(true)` so dropping the future never orphans the child.
pub fn command(name: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(resolve(name));
    match tool_dir() {
        Some(dir) => {
            let mut path = dir.into_os_string();
            path.push(":/usr/bin:/bin");
            cmd.env("PATH", path)
        }
        None => cmd.env("PATH", enriched_path()),
    };
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: don't flash a console window for each subprocess.
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enriched_path_keeps_existing_entries() {
        let current = std::env::var_os("PATH").unwrap_or_default();
        let enriched = enriched_path();
        let before: Vec<PathBuf> = std::env::split_paths(&current).collect();
        let after: Vec<PathBuf> = std::env::split_paths(&enriched).collect();
        assert!(after.len() >= before.len());
        assert_eq!(&after[..before.len()], &before[..]);
    }

    #[test]
    fn candidate_names_platform() {
        let names = candidate_names("helm");
        if cfg!(windows) {
            assert!(names.contains(&"helm.exe".to_string()));
            assert!(names.contains(&"helm.cmd".to_string()));
        } else {
            assert_eq!(names, vec!["helm".to_string()]);
        }
    }

    #[test]
    fn resolve_falls_back_to_name() {
        let p = resolve("definitely-not-a-real-tool-kore");
        assert_eq!(p, PathBuf::from("definitely-not-a-real-tool-kore"));
    }
}

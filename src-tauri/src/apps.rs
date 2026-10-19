//! Installed application discovery per OS.

use serde::Serialize;

#[derive(Serialize, Default)]
pub struct AppInfo {
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
    pub location: Option<String>,
    /// Size in bytes, when known.
    pub size: Option<u64>,
    /// Install date as reported by the OS (free-form, e.g. "20240131").
    pub installed: Option<String>,
    /// Last-used time in seconds since the Unix epoch, when known.
    pub last_used: Option<u64>,
}

#[cfg(windows)]
pub fn list() -> Vec<AppInfo> {
    use std::collections::HashSet;
    use winreg::RegKey;
    use winreg::enums::*;

    const PATHS: &[&str] = &[
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let root = RegKey::predef(hive);
        for p in PATHS {
            let Ok(key) = root.open_subkey(p) else { continue };
            for sub in key.enum_keys().flatten() {
                let Ok(k) = key.open_subkey(&sub) else { continue };
                let Ok(name) = k.get_value::<String, _>("DisplayName") else { continue };
                if k.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 {
                    continue;
                }
                let version: Option<String> = k.get_value("DisplayVersion").ok();
                if !seen.insert((name.clone(), version.clone())) {
                    continue;
                }
                let location: Option<String> = k
                    .get_value::<String, _>("InstallLocation")
                    .ok()
                    .filter(|s| !s.is_empty());
                out.push(AppInfo {
                    last_used: location.as_deref().and_then(last_used_in),
                    name,
                    version,
                    publisher: k.get_value("Publisher").ok(),
                    size: k
                        .get_value::<u32, _>("EstimatedSize")
                        .ok()
                        .map(|kb| kb as u64 * 1024),
                    installed: k.get_value("InstallDate").ok(),
                    location,
                });
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Best-effort "last used": the newest access time of executables in the
/// install folder (top level only, to stay cheap).
#[cfg(windows)]
fn last_used_in(dir: &str) -> Option<u64> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("exe"))
        })
        .filter_map(|e| e.metadata().ok()?.accessed().ok())
        .max()
        .and_then(to_epoch)
}

#[cfg(target_os = "macos")]
pub fn list() -> Vec<AppInfo> {
    use rayon::prelude::*;
    let mut dirs_ = vec![std::path::PathBuf::from("/Applications")];
    if let Some(h) = dirs::home_dir() {
        dirs_.push(h.join("Applications"));
    }
    let bundles: Vec<_> = dirs_
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "app"))
        .collect();
    let mut out: Vec<AppInfo> = bundles
        .par_iter()
        .map(|p| {
            let md = std::fs::metadata(p).ok();
            AppInfo {
                name: p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                location: Some(p.to_string_lossy().into_owned()),
                size: Some(crate::disk::dir_size(p).0),
                last_used: md.and_then(|m| m.accessed().ok()).and_then(to_epoch),
                ..Default::default()
            }
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn list() -> Vec<AppInfo> {
    let mut dirs_ = vec![
        std::path::PathBuf::from("/usr/share/applications"),
        std::path::PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    if let Some(d) = dirs::data_dir() {
        dirs_.push(d.join("applications"));
    }
    let mut out = Vec::new();
    for d in dirs_ {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let field = |k: &str| {
                text.lines()
                    .find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()))
            };
            if field("NoDisplay=").as_deref() == Some("true") {
                continue;
            }
            let Some(name) = field("Name=") else { continue };
            let exec = field("Exec=");
            let bin = exec
                .as_deref()
                .and_then(|e| e.split_whitespace().next())
                .map(|s| s.to_string());
            let bin_path = bin.as_deref().and_then(which);
            out.push(AppInfo {
                name,
                last_used: bin_path
                    .as_ref()
                    .and_then(|b| std::fs::metadata(b).ok()?.accessed().ok())
                    .and_then(to_epoch),
                location: bin_path.map(|b| b.to_string_lossy().into_owned()),
                ..Default::default()
            });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.name == b.name);
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
fn which(bin: &str) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(bin);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(bin))
            .find(|c| c.exists())
    })
}

fn to_epoch(t: std::time::SystemTime) -> Option<u64> {
    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

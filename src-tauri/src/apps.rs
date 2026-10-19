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
    /// Why it can't be removed from here (built into the OS…); `None` when it can.
    pub blocked: Option<String>,
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
    let runs = user_assist();
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
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        // DisplayIcon looks like "C:\App\app.exe,0"; use its folder.
                        let icon: String = k.get_value("DisplayIcon").ok()?;
                        let exe = icon.split(',').next()?.trim().trim_matches('"').to_string();
                        let dir = std::path::Path::new(&exe).parent()?;
                        (dir.components().count() > 2).then(|| dir.to_string_lossy().into_owned())
                    });
                let has_uninstaller = k.get_value::<String, _>("UninstallString").is_ok_and(|u| !u.trim().is_empty());
                let from_runs = runs.last_run(&name, location.as_deref());
                let from_files = location.as_deref().and_then(last_used_in);
                out.push(AppInfo {
                    last_used: from_runs.max(from_files),
                    name,
                    version,
                    publisher: k.get_value("Publisher").ok(),
                    size: k
                        .get_value::<u32, _>("EstimatedSize")
                        .ok()
                        .map(|kb| kb as u64 * 1024),
                    installed: k.get_value("InstallDate").ok(),
                    location,
                    blocked: (!has_uninstaller).then(|| "no uninstaller registered".to_string()),
                });
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// The registered uninstall command for the app with this display name.
#[cfg(windows)]
pub fn uninstall_command(name: &str) -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::*;
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for p in [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let Ok(key) = RegKey::predef(hive).open_subkey(p) else { continue };
            for sub in key.enum_keys().flatten() {
                let Ok(k) = key.open_subkey(&sub) else { continue };
                if k.get_value::<String, _>("DisplayName").ok().as_deref() == Some(name) {
                    if let Ok(cmd) = k.get_value::<String, _>("UninstallString") {
                        return Some(cmd);
                    }
                }
            }
        }
    }
    None
}

/// Last-run times recorded by Explorer in the UserAssist registry key
/// (ROT13-encoded program paths, FILETIME at offset 60 of each value).
#[cfg(windows)]
struct Runs {
    by_path: Vec<(String, u64)>,
    by_shortcut: std::collections::HashMap<String, u64>,
}

#[cfg(windows)]
impl Runs {
    fn last_run(&self, name: &str, location: Option<&str>) -> Option<u64> {
        let by_loc = location.and_then(|loc| {
            let loc = loc.trim_end_matches('\\').to_lowercase() + "\\";
            self.by_path
                .iter()
                .filter(|(p, _)| p.starts_with(&loc))
                .map(|(_, t)| *t)
                .max()
        });
        let by_name = self.by_shortcut.get(&name.to_lowercase()).copied();
        by_loc.max(by_name)
    }
}

#[cfg(windows)]
fn user_assist() -> Runs {
    use winreg::RegKey;
    use winreg::enums::*;

    let mut runs = Runs {
        by_path: Vec::new(),
        by_shortcut: Default::default(),
    };
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let win = env("SystemRoot", r"C:\Windows");
    // Known-folder GUIDs Explorer uses as path prefixes.
    let folders = [
        ("{6D809377-6AF0-444B-8957-A3773F02200E}", env("ProgramW6432", r"C:\Program Files")),
        ("{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}", env("ProgramFiles(x86)", r"C:\Program Files (x86)")),
        ("{F38BF404-1D43-42F2-9305-67DE0B28FC23}", win.clone()),
        ("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}", format!(r"{win}\System32")),
        ("{D65231B0-B2F1-4857-A4CE-A8E7C6EA7D27}", format!(r"{win}\SysWOW64")),
    ];
    let Ok(ua) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist")
    else {
        return runs;
    };
    for guid in ua.enum_keys().flatten() {
        let Ok(count) = ua.open_subkey(format!(r"{guid}\Count")) else { continue };
        for (name, value) in count.enum_values().flatten() {
            if value.bytes.len() < 68 {
                continue;
            }
            let ft = u64::from_le_bytes(value.bytes[60..68].try_into().unwrap());
            if ft == 0 {
                continue;
            }
            let secs = (ft / 10_000_000).saturating_sub(11_644_473_600);
            let mut path = rot13(&name);
            for (g, dir) in &folders {
                if let Some(rest) = path.strip_prefix(g) {
                    path = format!("{dir}{rest}");
                }
            }
            let lower = path.to_lowercase();
            if lower.ends_with(".lnk") {
                if let Some(stem) = std::path::Path::new(&lower).file_stem() {
                    let e = runs.by_shortcut.entry(stem.to_string_lossy().into_owned()).or_default();
                    *e = (*e).max(secs);
                }
            } else {
                runs.by_path.push((lower, secs));
            }
        }
    }
    runs
}

#[cfg(windows)]
fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a') + 13) % 26 + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A') + 13) % 26 + b'A') as char,
            _ => c,
        })
        .collect()
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
            let id = crate::uninstall::bundle_id(p);
            AppInfo {
                blocked: crate::uninstall::blocked(p, id.as_deref()),
                publisher: id.as_deref().filter(|i| i.starts_with("com.apple.")).map(|_| "Apple".to_string()),
                name: p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                location: Some(p.to_string_lossy().into_owned()),
                size: Some(crate::disk::dir_size(p).0),
                last_used: spotlight_last_used(p)
                    .or_else(|| md.and_then(|m| m.accessed().ok()).and_then(to_epoch)),
                ..Default::default()
            }
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Reads Spotlight's last-used date ("2024-01-31 09:15:00 +0000").
#[cfg(target_os = "macos")]
fn spotlight_last_used(p: &std::path::Path) -> Option<u64> {
    let out = std::process::Command::new("mdls")
        .args(["-raw", "-name", "kMDItemLastUsedDate"])
        .arg(p)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let mut it = s.split(|c: char| c == '-' || c == ' ' || c == ':');
    let mut n = || it.next()?.parse::<i64>().ok();
    let (y, m, d, hh, mm, ss) = (n()?, n()?, n()?, n()?, n()?, n()?);
    // Days from civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
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

#[cfg(test)]
mod tests {
    #[test]
    fn lists_apps() {
        let apps = super::list();
        let dated = apps.iter().filter(|a| a.last_used.is_some()).count();
        println!("{} apps, {} with a last-used date", apps.len(), dated);
        #[cfg(windows)]
        assert_eq!(super::rot13("Nccf"), "Apps");
    }
}

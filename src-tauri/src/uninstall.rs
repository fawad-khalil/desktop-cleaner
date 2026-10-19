//! Removing applications. Nothing here deletes permanently:
//! - macOS: the .app bundle (and any leftovers the user ticks) go to the Trash.
//! - Windows: the app's own registered uninstaller is launched.
//! - Linux: the package-manager command is returned for the user to run.
//!
//! Apps that are part of macOS (System Integrity Protection), anything
//! outside the Applications folders, and this app itself are refused.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct Leftover {
    pub path: String,
    pub size: u64,
}

#[derive(Serialize, Default)]
pub struct Outcome {
    /// Paths moved to the Trash (macOS).
    pub moved: Vec<String>,
    pub freed: u64,
    /// Paths that couldn't be moved, with the reason.
    pub failed: Vec<(String, String)>,
    /// The app's uninstaller was started (Windows).
    pub launched: bool,
    /// A command for the user to run themselves (Linux).
    pub command: Option<String>,
}

/// Why an app can't be removed from here, or `None` if it can.
#[cfg(target_os = "macos")]
pub fn blocked(location: &Path, bundle_id: Option<&str>) -> Option<String> {
    if location.extension().is_none_or(|e| e != "app") {
        return Some("not an app bundle".into());
    }
    let parent = location.parent()?;
    let home_apps = dirs::home_dir().map(|h| h.join("Applications"));
    if parent != Path::new("/Applications") && Some(parent.to_path_buf()) != home_apps {
        return Some("only apps directly in an Applications folder can be removed".into());
    }
    // Safari and the like are SIP-protected links into the system volume.
    // Apple apps from the App Store (Pages, Xcode…) are ordinary bundles.
    use std::os::macos::fs::MetadataExt;
    const SF_RESTRICTED: u32 = 0x0008_0000;
    match std::fs::symlink_metadata(location) {
        Ok(m) if m.file_type().is_symlink() || m.st_flags() & SF_RESTRICTED != 0 => {
            return Some("built into macOS".into());
        }
        Ok(_) => {}
        Err(e) => return Some(e.to_string()),
    }
    let _ = bundle_id;
    if let Ok(exe) = std::env::current_exe() {
        if exe.starts_with(location) {
            return Some("this is DesktopCleaner itself".into());
        }
    }
    None
}

/// One string value from an app's Info.plist.
#[cfg(target_os = "macos")]
pub fn plist_value(app: &Path, key: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !v.is_empty()).then_some(v)
}

#[cfg(target_os = "macos")]
pub fn bundle_id(app: &Path) -> Option<String> {
    plist_value(app, "CFBundleIdentifier")
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// "© 2005–2024 Apple Inc. All rights reserved." → "Apple Inc."
pub fn publisher_from_copyright(c: &str) -> Option<String> {
    let mut s = c.trim();
    for p in ["Copyright", "copyright", "(c)", "(C)", "©"] {
        s = s.trim_start_matches(p).trim_start();
    }
    let s = s.trim_start_matches(|ch: char| ch.is_ascii_digit() || "-–, ".contains(ch)).trim_start_matches('©').trim();
    let lower = s.to_ascii_lowercase();
    let end = ["all rights", ", a ", ". licensed"].iter().filter_map(|m| lower.find(m)).min().unwrap_or(s.len());
    let name = s[..end].trim().trim_end_matches([',', '.']).trim();
    (!name.is_empty() && name.len() <= 60).then(|| name.to_string())
}

/// Per-user support files an app usually leaves behind, found by bundle id
/// or app name in the standard ~/Library folders. Containers and anything
/// the safety rules protect are never offered.
#[cfg(target_os = "macos")]
pub fn leftovers(location: &str) -> Vec<Leftover> {
    let app = Path::new(location);
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    let name = app.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let id = bundle_id(app);
    if name.len() < 3 {
        return Vec::new();
    }
    let lib = home.join("Library");
    let mut keys: Vec<String> = vec![name.clone()];
    if let Some(id) = &id {
        keys.push(id.clone());
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in ["Caches", "Application Support", "Preferences", "Saved Application State", "Logs", "HTTPStorages", "WebKit"] {
        let Ok(rd) = std::fs::read_dir(lib.join(dir)) else { continue };
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().into_owned();
            // Exact name, or the bundle id followed by an extension/suffix
            // (com.foo.App.plist, com.foo.App.savedState…).
            let hit = keys.iter().any(|k| fname == *k || fname.starts_with(&format!("{k}.")));
            if hit {
                found.push(e.path());
            }
        }
    }
    found
        .into_iter()
        .filter(|p| crate::safety::check(p) == crate::safety::Verdict::Allowed)
        .map(|p| {
            let size = match std::fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => crate::disk::dir_size(&p).0,
                Ok(m) => m.len(),
                Err(_) => 0,
            };
            Leftover { path: p.to_string_lossy().into_owned(), size }
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub fn remove(location: &str, extra: &[String]) -> anyhow::Result<Outcome> {
    let app = PathBuf::from(location);
    if let Some(why) = blocked(&app, bundle_id(&app).as_deref()) {
        anyhow::bail!("{} can't be removed: {why}", app.display());
    }
    // Leftovers must be ones we'd offer for this app.
    let allowed: Vec<String> = leftovers(location).into_iter().map(|l| l.path).collect();
    let mut out = Outcome::default();
    let mut items = vec![app.clone()];
    for e in extra {
        if allowed.contains(e) {
            items.push(PathBuf::from(e));
        } else {
            out.failed.push((e.clone(), "not a leftover of this app".into()));
        }
    }
    for p in items {
        let size = crate::disk::dir_size(&p).0.max(std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0));
        match trash::delete(&p) {
            Ok(()) => {
                out.moved.push(p.to_string_lossy().into_owned());
                out.freed += size;
            }
            Err(e) => out.failed.push((p.to_string_lossy().into_owned(), e.to_string())),
        }
    }
    Ok(out)
}

#[cfg(windows)]
pub fn leftovers(_location: &str) -> Vec<Leftover> {
    Vec::new()
}

/// Starts the uninstaller registered for the app named `name`.
#[cfg(windows)]
pub fn remove(name: &str, _extra: &[String]) -> anyhow::Result<Outcome> {
    let cmd = crate::apps::uninstall_command(name).ok_or_else(|| anyhow::anyhow!("{name} has no registered uninstaller"))?;
    // The uninstaller shows its own window and asks for elevation itself.
    std::process::Command::new("cmd").args(["/C", &format!("start \"\" {cmd}")]).spawn()?;
    Ok(Outcome { launched: true, ..Default::default() })
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn leftovers(_location: &str) -> Vec<Leftover> {
    Vec::new()
}

/// Works out the package-manager command that removes the app owning
/// `location` (its executable); the user runs it themselves.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn remove(location: &str, _extra: &[String]) -> anyhow::Result<Outcome> {
    use std::process::Command;
    let run = |prog: &str, args: &[&str]| {
        Command::new(prog)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let command = if let Some(snap) = location.strip_prefix("/snap/bin/") {
        Some(format!("sudo snap remove {}", snap.split('.').next().unwrap_or(snap)))
    } else if location.contains("/flatpak/") {
        let id = Path::new(location).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        Some(format!("flatpak uninstall {id}"))
    } else if let Some(pkg) = run("dpkg", &["-S", location]).and_then(|s| s.split(':').next().map(str::to_string)) {
        Some(format!("sudo apt remove {pkg}"))
    } else {
        run("rpm", &["-qf", "--qf", "%{NAME}", location]).map(|pkg| format!("sudo dnf remove {pkg}"))
    };
    let command = command.ok_or_else(|| anyhow::anyhow!("couldn't tell which package installed {location}"))?;
    Ok(Outcome { command: Some(command), ..Default::default() })
}

#[cfg(test)]
mod tests {
    #[test]
    fn publisher_from_copyright_lines() {
        use super::publisher_from_copyright as p;
        assert_eq!(p("© 2005–2024 Apple Inc. All rights reserved.").as_deref(), Some("Apple Inc"));
        assert_eq!(p("©2025 Slack Technologies LLC, a Salesforce company. All rights reserved.").as_deref(), Some("Slack Technologies LLC"));
        assert_eq!(p("Copyright 2019 Foo GmbH").as_deref(), Some("Foo GmbH"));
        assert_eq!(p("Copyright © 2024 Docker Inc.All Rights Reserved").as_deref(), Some("Docker Inc"));
        assert_eq!(p("©").as_deref(), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn refuses_apple_and_odd_locations() {
        use super::*;
        let safari = Path::new("/Applications/Safari.app");
        if safari.exists() {
            assert_eq!(blocked(safari, None).as_deref(), Some("built into macOS"));
        }
        assert!(blocked(Path::new("/System/Applications/Notes.app"), None).is_some());
        assert!(blocked(Path::new("/Applications/Sub/Thing.app"), None).is_some());
        assert!(blocked(Path::new("/Applications/Thing"), None).is_some());
        // App Store apps from Apple are ordinary bundles and removable.
        let pages = Path::new("/Applications/Pages.app");
        if pages.exists() {
            assert_eq!(blocked(pages, None), None);
        }
        // remove() re-checks and refuses without touching anything.
        assert!(remove("/Applications/Safari.app", &[]).is_err());
        assert!(safari.exists());
    }
}

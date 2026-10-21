//! Well-known cache locations per OS and their sizes.

use rayon::prelude::*;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct CacheInfo {
    pub name: String,
    pub group: String,
    pub path: String,
    pub size: u64,
    pub files: u64,
    /// macOS protects it and the app doesn't have Full Disk Access, so it
    /// wasn't read (size and files are 0).
    pub needs_access: bool,
}

fn candidates() -> Vec<(&'static str, &'static str, PathBuf)> {
    let mut v: Vec<(&'static str, &'static str, PathBuf)> = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();
    let cache = dirs::cache_dir();
    let mut push = |name, group, p: Option<PathBuf>| {
        if let Some(p) = p {
            v.push((name, group, p));
        }
    };

    push("User temp folder", "System temp", Some(std::env::temp_dir()));

    #[cfg(windows)]
    {
        let local = dirs::data_local_dir();
        let roaming = dirs::data_dir();
        let lj = |s: &str| local.as_ref().map(|l| l.join(s));
        push("Internet cache (INetCache)", "System temp", lj(r"Microsoft\Windows\INetCache"));
        push("Explorer thumbnail cache", "System temp", lj(r"Microsoft\Windows\Explorer"));
        push("Crash dumps", "Logs", lj("CrashDumps"));
        push("Windows Error Reporting", "Logs", lj(r"Microsoft\Windows\WER"));
        push("Chrome cache", "Browsers", lj(r"Google\Chrome\User Data\Default\Cache"));
        push("Chrome code cache", "Browsers", lj(r"Google\Chrome\User Data\Default\Code Cache"));
        push("Edge cache", "Browsers", lj(r"Microsoft\Edge\User Data\Default\Cache"));
        push("Edge code cache", "Browsers", lj(r"Microsoft\Edge\User Data\Default\Code Cache"));
        push("Brave cache", "Browsers", lj(r"BraveSoftware\Brave-Browser\User Data\Default\Cache"));
        push("Firefox cache", "Browsers", lj(r"Mozilla\Firefox\Profiles"));
        push("npm cache", "Developer", lj("npm-cache"));
        push("pip cache", "Developer", lj(r"pip\cache"));
        push("Yarn cache", "Developer", lj(r"Yarn\Cache"));
        push("NuGet cache", "Developer", Some(home.join(r".nuget\packages")));
        push("Gradle cache", "Developer", Some(home.join(r".gradle\caches")));
        push("Cargo registry cache", "Developer", Some(home.join(r".cargo\registry\cache")));
        push("Teams cache", "Apps", roaming.as_ref().map(|r| r.join(r"Microsoft\Teams\Cache")));
        push("Discord cache", "Apps", roaming.as_ref().map(|r| r.join(r"discord\Cache")));
        push("Slack cache", "Apps", roaming.as_ref().map(|r| r.join(r"Slack\Cache")));
        push("Spotify cache", "Apps", lj(r"Spotify\Data"));
        push("Windows temp (system)", "System temp", Some(PathBuf::from(r"C:\Windows\Temp")));
    }

    #[cfg(target_os = "macos")]
    {
        push("User caches", "System temp", cache.clone());
        push("User logs", "Logs", Some(home.join("Library/Logs")));
        push("Safari cache", "Browsers", Some(home.join("Library/Caches/com.apple.Safari")));
        push("Chrome cache", "Browsers", Some(home.join("Library/Caches/Google/Chrome")));
        push("Xcode DerivedData", "Developer", Some(home.join("Library/Developer/Xcode/DerivedData")));
        push("npm cache", "Developer", Some(home.join(".npm/_cacache")));
        push("Homebrew cache", "Developer", Some(home.join("Library/Caches/Homebrew")));
        push("Cargo registry cache", "Developer", Some(home.join(".cargo/registry/cache")));
        push("Gradle cache", "Developer", Some(home.join(".gradle/caches")));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        push("User cache (~/.cache)", "System temp", cache.clone());
        push("Thumbnails", "System temp", cache.as_ref().map(|c| c.join("thumbnails")));
        push("Chrome cache", "Browsers", cache.as_ref().map(|c| c.join("google-chrome")));
        push("Firefox cache", "Browsers", cache.as_ref().map(|c| c.join("mozilla")));
        push("npm cache", "Developer", Some(home.join(".npm/_cacache")));
        push("pip cache", "Developer", cache.as_ref().map(|c| c.join("pip")));
        push("Cargo registry cache", "Developer", Some(home.join(".cargo/registry/cache")));
        push("Gradle cache", "Developer", Some(home.join(".gradle/caches")));
        push("Trash", "System temp", dirs::data_dir().map(|d| d.join("Trash")));
        push("APT package cache (system)", "System temp", Some(PathBuf::from("/var/cache/apt/archives")));
        push("System journal logs", "Logs", Some(PathBuf::from("/var/log/journal")));
    }

    let _ = &cache;
    v
}

pub fn list() -> Vec<CacheInfo> {
    let mut out: Vec<CacheInfo> = candidates()
        .into_par_iter()
        .filter(|(_, _, p)| p.is_dir())
        .map(|(name, group, p)| {
            let needs_access = crate::privacy::withheld(&p);
            let (size, files) = if needs_access { (0, 0) } else { crate::disk::dir_size(&p) };
            CacheInfo {
                needs_access,
                name: name.to_string(),
                group: group.to_string(),
                path: p.to_string_lossy().into_owned(),
                size,
                files,
            }
        })
        .collect();
    out.sort_by(|a, b| b.size.cmp(&a.size));
    out
}

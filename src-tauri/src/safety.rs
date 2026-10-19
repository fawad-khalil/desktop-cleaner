//! Protected-path rules. Every path is checked here before it is offered for
//! cleanup *and* again right before it is deleted. The rules are deliberately
//! conservative: when in doubt, a path is protected.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Protected(String),
}

impl Verdict {
    #[allow(dead_code)] // used by tests
    pub fn is_allowed(&self) -> bool {
        matches!(self, Verdict::Allowed)
    }
}

/// Normalises a path for comparison: absolute, no `.`/`..`, no `\\?\`
/// prefix, lowercase on case-insensitive platforms.
fn normalise(p: &Path) -> String {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    let mut s = out.to_string_lossy().replace('\\', "/");
    if let Some(rest) = s.strip_prefix("//?/") {
        s = rest.to_string();
    }
    while s.len() > 1 && s.ends_with('/') && !s.ends_with(":/") {
        s.pop();
    }
    if cfg!(any(windows, target_os = "macos")) {
        s = s.to_lowercase();
    }
    s
}

fn under(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!("{}/", root.trim_end_matches('/')))
}

/// Directories whose *contents* may be cleaned even though they sit inside a
/// protected tree.
fn exceptions() -> Vec<String> {
    // The per-user temp dir lives under /var on macOS.
    let mut v: Vec<PathBuf> = vec![std::env::temp_dir()];
    #[cfg(windows)]
    {
        let win = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or(r"C:\Windows".into());
        v.push(win.join("Temp"));
        v.push(win.join("SoftwareDistribution").join("Download"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        v.push("/var/cache/apt/archives".into());
        v.push("/var/tmp".into());
        v.push("/var/log/journal".into());
    }
    #[cfg(target_os = "macos")]
    {
        v.push("/private/var/tmp".into());
    }
    v.iter().map(|p| normalise(p)).collect()
}

/// Trees that are never touched.
fn protected_roots() -> Vec<String> {
    let mut v: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        let env = |k: &str, d: &str| std::env::var_os(k).map(PathBuf::from).unwrap_or(d.into());
        let drive = env("SystemDrive", "C:");
        let drive = PathBuf::from(format!("{}\\", drive.to_string_lossy()));
        v.push(env("SystemRoot", r"C:\Windows"));
        v.push(env("ProgramFiles", r"C:\Program Files"));
        v.push(env("ProgramFiles(x86)", r"C:\Program Files (x86)"));
        v.push(env("ProgramW6432", r"C:\Program Files"));
        v.push(env("ProgramData", r"C:\ProgramData"));
        for name in [
            "System Volume Information",
            "$Recycle.Bin",
            "Recovery",
            "Boot",
            "EFI",
            "$WinREAgent",
            "Config.Msi",
            "PerfLogs",
            "Documents and Settings",
            "pagefile.sys",
            "hiberfil.sys",
            "swapfile.sys",
            "DumpStack.log.tmp",
            "bootmgr",
            "BOOTNXT",
        ] {
            v.push(drive.join(name));
        }
        if let Some(local) = dirs::data_local_dir() {
            v.push(local.join("Microsoft").join("WindowsApps"));
            v.push(local.join("Packages"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        for p in [
            "/System", "/Library", "/bin", "/sbin", "/usr", "/etc", "/var", "/private",
            "/cores", "/dev", "/Volumes", "/opt", "/Applications",
        ] {
            v.push(p.into());
        }
        if let Some(h) = dirs::home_dir() {
            v.push(h.join("Library/Keychains"));
            v.push(h.join("Library/Mobile Documents"));
            v.push(h.join("Library/Containers"));
            v.push(h.join("Library/Group Containers"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for p in [
            "/bin", "/boot", "/dev", "/etc", "/lib", "/lib32", "/lib64", "/libx32", "/proc",
            "/sys", "/usr", "/var", "/run", "/sbin", "/snap", "/root", "/opt", "/srv",
            "/lost+found", "/efi", "/media", "/mnt",
        ] {
            v.push(p.into());
        }
    }
    // Credentials and keys in the user's home are never cleanup material.
    if let Some(h) = dirs::home_dir() {
        for name in [".ssh", ".gnupg", ".aws", ".kube", ".docker", ".config/gcloud"] {
            v.push(h.join(name));
        }
    }
    v.iter().map(|p| normalise(p)).collect()
}

/// Paths that must never be deleted *themselves* (their contents may be).
fn anchor_dirs() -> Vec<String> {
    let mut v: Vec<PathBuf> = vec![std::env::temp_dir()];
    v.extend(
        [
            dirs::home_dir(),
            dirs::cache_dir(),
            dirs::data_dir(),
            dirs::data_local_dir(),
            dirs::config_dir(),
            dirs::desktop_dir(),
            dirs::document_dir(),
            dirs::download_dir(),
            dirs::picture_dir(),
            dirs::video_dir(),
            dirs::audio_dir(),
        ]
        .into_iter()
        .flatten(),
    );
    if let Some(h) = dirs::home_dir() {
        if let Some(parent) = h.parent() {
            v.push(parent.to_path_buf()); // C:\Users, /home, /Users
        }
    }
    v.iter().map(|p| normalise(p)).collect()
}

/// Decides whether `path` may be deleted. Does not touch the filesystem
/// except for reading metadata.
pub fn check(path: &Path) -> Verdict {
    if !path.is_absolute() {
        return Verdict::Protected("not an absolute path".into());
    }
    let n = normalise(path);

    // Drive / filesystem roots.
    let depth = n.trim_end_matches('/').split('/').filter(|s| !s.is_empty()).count();
    if depth <= 1 {
        return Verdict::Protected("drive or filesystem root".into());
    }
    if anchor_dirs().iter().any(|a| *a == n) {
        return Verdict::Protected("top-level user or system folder".into());
    }

    let excepted = exceptions().iter().any(|e| under(&n, e) && n != *e);
    if !excepted {
        if let Some(root) = protected_roots().iter().find(|r| under(&n, r)) {
            return Verdict::Protected(format!("inside protected location {root}"));
        }
    }

    if let Ok(md) = std::fs::symlink_metadata(path) {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
            if md.file_attributes() & FILE_ATTRIBUTE_SYSTEM != 0 {
                return Verdict::Protected("marked as a system file".into());
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // Files owned by root outside the excepted caches are system files.
            if md.uid() == 0 && !excepted {
                return Verdict::Protected("owned by root".into());
            }
        }
        let _ = &md;
    }
    Verdict::Allowed
}

/// Whether a folder may be *searched* for cleanup candidates. Looser than
/// [`check`]: home and drive roots are fine to search (each item found is
/// still checked individually), protected system trees are not.
pub fn can_scan(path: &Path) -> Verdict {
    if !path.is_absolute() {
        return Verdict::Protected("not an absolute path".into());
    }
    let n = normalise(path);
    if exceptions().iter().any(|e| under(&n, e)) {
        return Verdict::Allowed;
    }
    match protected_roots().iter().find(|r| under(&n, r)) {
        Some(root) => Verdict::Protected(format!("inside protected location {root}")),
        None => Verdict::Allowed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_and_anchors_are_protected() {
        let home = dirs::home_dir().unwrap();
        assert!(!check(&home).is_allowed());
        assert!(!check(&std::env::temp_dir()).is_allowed());
        assert!(!check(Path::new("relative/path")).is_allowed());
        #[cfg(windows)]
        assert!(!check(Path::new(r"C:\")).is_allowed());
        #[cfg(unix)]
        assert!(!check(Path::new("/")).is_allowed());
    }

    #[cfg(windows)]
    #[test]
    fn windows_system_paths_are_protected() {
        for p in [
            r"C:\Windows\System32\kernel32.dll",
            r"c:\windows\system32",
            r"C:\Program Files\App\app.exe",
            r"C:\ProgramData\foo",
            r"C:\pagefile.sys",
            r"C:\Users\x\..\..\Windows\notepad.exe",
            r"\\?\C:\Windows\explorer.exe",
            r"C:\Windows\Temp",
        ] {
            assert!(!check(Path::new(p)).is_allowed(), "{p} should be protected");
        }
        assert!(check(Path::new(r"C:\Windows\Temp\some-installer.log")).is_allowed());
    }

    #[cfg(unix)]
    #[test]
    fn unix_system_paths_are_protected() {
        for p in ["/usr/bin/ls", "/etc/passwd", "/var/lib/dpkg", "/opt/tool"] {
            assert!(!check(Path::new(p)).is_allowed(), "{p} should be protected");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_system_paths_are_protected() {
        for p in ["/boot/vmlinuz", "/proc/1", "/sys/kernel", "/snap/core"] {
            assert!(!check(Path::new(p)).is_allowed(), "{p} should be protected");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_system_paths_are_protected() {
        for p in [
            "/System/Library/Kernels/kernel",
            "/Library/Preferences/com.apple.loginwindow.plist",
            "/Applications/Safari.app",
            "/private/etc/hosts",
            "/usr/lib/dyld",
            "/system/library",
        ] {
            assert!(!check(Path::new(p)).is_allowed(), "{p} should be protected");
        }
        let keychains = dirs::home_dir().unwrap().join("Library/Keychains/login.keychain-db");
        assert!(!check(&keychains).is_allowed());
    }

    #[test]
    fn user_files_are_allowed() {
        let p = std::env::temp_dir().join("sc-some-temp-file.tmp");
        assert!(check(&p).is_allowed());
        let ssh = dirs::home_dir().unwrap().join(".ssh").join("id_rsa");
        assert!(!check(&ssh).is_allowed());
    }
}

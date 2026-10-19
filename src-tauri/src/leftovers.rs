//! Leftovers from OS updates and installers: old OS copies, upgrade staging
//! folders, extracted driver packages, old kernels, stale backups.
//!
//! These always need explicit confirmation. Items owned by the OS are never
//! deleted by us: we point the user to the OS's own tool or the exact
//! command instead.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // not every variant is used on every OS
pub enum Action {
    /// We can move it to the Recycle Bin/Trash after confirmation.
    Trash,
    /// Must be removed with the OS's own cleanup tool, which we can open.
    SystemTool,
    /// Must be removed with a package-manager command we show (never run).
    Command,
    /// Must be removed by hand (e.g. in Finder) because it is in a protected area.
    Manual,
}

#[derive(Serialize)]
pub struct Leftover {
    pub title: String,
    pub description: String,
    pub paths: Vec<String>,
    pub size: u64,
    pub action: Action,
    pub command: Option<String>,
}

fn size_of(paths: &[PathBuf]) -> u64 {
    use rayon::prelude::*;
    paths
        .par_iter()
        .map(|p| match std::fs::symlink_metadata(p) {
            Ok(m) if m.is_dir() => crate::disk::dir_size(p).0,
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

fn item(title: &str, description: &str, paths: Vec<PathBuf>, action: Action, command: Option<&str>) -> Option<Leftover> {
    let paths: Vec<PathBuf> = paths.into_iter().filter(|p| p.exists()).collect();
    if paths.is_empty() {
        return None;
    }
    Some(Leftover {
        title: title.into(),
        description: description.into(),
        size: size_of(&paths),
        paths: paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        action,
        command: command.map(String::from),
    })
}

#[cfg(windows)]
pub fn find() -> Vec<Leftover> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let root = PathBuf::from(format!("{drive}\\"));
    let win = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or(root.join("Windows"));
    let mut v = vec![
        item(
            "Previous Windows installation (Windows.old)",
            "A full copy of your previous Windows version, kept after a feature update so you can roll back. \
             Once you're happy with the current version it can go. Windows only allows its own Disk Cleanup \
             tool to remove it (\"Previous Windows installation(s)\").",
            vec![root.join("Windows.old")],
            Action::SystemTool,
            None,
        ),
        item(
            "Windows upgrade staging files",
            "Temporary files left by Windows feature upgrades ($Windows.~BT, $Windows.~WS, ESD). \
             Removed with Disk Cleanup (\"Temporary Windows installation files\").",
            vec![root.join("$Windows.~BT"), root.join("$Windows.~WS"), root.join("$WINDOWS.~Q"), root.join("ESD")],
            Action::SystemTool,
            None,
        ),
        item(
            "Downloaded Windows Update packages",
            "Update packages that have already been installed. Windows downloads them again if needed. \
             Deleting needs administrator rights; items that are in use are skipped.",
            vec![win.join("SoftwareDistribution").join("Download")],
            Action::Trash,
            None,
        ),
        item(
            "Windows Update Assistant leftovers",
            "Folders left by the Windows 10/11 Update or Installation Assistant after an upgrade.",
            vec![root.join("Windows10Upgrade"), root.join("Windows11InstallationAssistant")],
            Action::Trash,
            None,
        ),
        item(
            "Extracted driver installers",
            "Driver setup files that GPU and chipset installers unpack and leave behind. Installed drivers are \
             not affected, but you would need to download the installer again to reinstall.",
            vec![root.join("NVIDIA"), root.join("AMD"), root.join("Intel")],
            Action::Trash,
            None,
        ),
    ];
    if let Some(local) = dirs::data_local_dir() {
        v.push(item(
            "Old Windows error reports",
            "Crash reports that have already been sent or archived.",
            vec![local.join(r"Microsoft\Windows\WER\ReportArchive"), local.join(r"Microsoft\Windows\WER\ReportQueue")],
            Action::Trash,
            None,
        ));
    }
    v.into_iter().flatten().collect()
}

#[cfg(windows)]
pub fn open_system_tool() -> anyhow::Result<()> {
    // Disk Cleanup in "system files" mode: the only supported way to remove
    // Windows.old. The user picks what to remove in Windows' own UI.
    std::process::Command::new("cleanmgr.exe").arg("/d").arg(std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into())).spawn()?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn find() -> Vec<Leftover> {
    let mut v = Vec::new();
    let current = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let current = current.trim();
    let mut old: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/lib/modules") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !current.is_empty() && name != current {
                old.push(e.path());
                for f in ["vmlinuz", "initrd.img", "initramfs", "System.map", "config"] {
                    old.push(PathBuf::from(format!("/boot/{f}-{name}")));
                    old.push(PathBuf::from(format!("/boot/{f}-{name}.img")));
                }
            }
        }
    }
    let pm = package_manager();
    let kernel_cmd = match pm {
        "apt" => "sudo apt autoremove --purge",
        "dnf" => "sudo dnf remove --oldinstallonly",
        "pacman" => "Arch keeps only the installed kernel; check `pacman -Q | grep linux`",
        "zypper" => "sudo zypper purge-kernels",
        _ => "Remove old kernel packages with your package manager",
    };
    v.push(item(
        "Old kernels",
        &format!("Kernels other than the one you're running ({current}). Keep at least one older kernel as a fallback if you're unsure."),
        old,
        Action::Command,
        Some(kernel_cmd),
    ));
    let (cache, cache_cmd) = match pm {
        "apt" => ("/var/cache/apt/archives", "sudo apt clean"),
        "dnf" => ("/var/cache/dnf", "sudo dnf clean all"),
        "pacman" => ("/var/cache/pacman/pkg", "sudo paccache -r"),
        "zypper" => ("/var/cache/zypp/packages", "sudo zypper clean"),
        _ => ("/var/cache/apt/archives", "Clean the package cache with your package manager"),
    };
    v.push(item(
        "Downloaded package files",
        "Package files kept after installing updates.",
        vec![PathBuf::from(cache)],
        Action::Command,
        Some(cache_cmd),
    ));
    v.push(item(
        "Old system logs",
        "Systemd journal files. Vacuuming keeps the last two weeks.",
        vec![PathBuf::from("/var/log/journal")],
        Action::Command,
        Some("sudo journalctl --vacuum-time=2weeks"),
    ));
    v.push(item(
        "Disabled snap revisions",
        "Snap keeps old revisions of each package after an update.",
        vec![PathBuf::from("/var/lib/snapd/snaps")],
        Action::Command,
        Some("snap list --all | awk '/disabled/{print $1, $3}' | while read n r; do sudo snap remove \"$n\" --revision=\"$r\"; done"),
    ));
    v.into_iter().flatten().collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn package_manager() -> &'static str {
    for (bin, name) in [("/usr/bin/apt", "apt"), ("/usr/bin/dnf", "dnf"), ("/usr/bin/pacman", "pacman"), ("/usr/bin/zypper", "zypper")] {
        if std::path::Path::new(bin).exists() {
            return name;
        }
    }
    "unknown"
}

#[cfg(target_os = "macos")]
pub fn find() -> Vec<Leftover> {
    let home = dirs::home_dir().unwrap_or_default();
    let installers: Vec<PathBuf> = std::fs::read_dir("/Applications")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("Install macOS")))
                .collect()
        })
        .unwrap_or_default();
    vec![
        item(
            "macOS installers",
            "Full macOS installer apps left after an upgrade. Drag them to the Trash in Finder.",
            installers,
            Action::Manual,
            None,
        ),
        item(
            "Old iPhone / iPad backups",
            "Local device backups made by Finder or iTunes. Make sure you still have a recent backup before removing.",
            vec![home.join("Library/Application Support/MobileSync/Backup")],
            Action::Trash,
            None,
        ),
        item(
            "iOS software updates",
            "Downloaded iOS firmware files (.ipsw).",
            vec![home.join("Library/iTunes/iPhone Software Updates"), home.join("Library/iTunes/iPad Software Updates")],
            Action::Trash,
            None,
        ),
        item(
            "Downloaded macOS updates",
            "Update packages already installed. Removed by macOS itself, or manually with administrator rights.",
            vec![PathBuf::from("/Library/Updates")],
            Action::Manual,
            None,
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(not(windows))]
pub fn open_system_tool() -> anyhow::Result<()> {
    anyhow::bail!("no system cleanup tool on this platform")
}

#[cfg(test)]
mod tests {
    #[test]
    fn find_only_reports_existing_paths() {
        for l in super::find() {
            assert!(!l.paths.is_empty());
            for p in &l.paths {
                assert!(std::path::Path::new(p).exists(), "{p}");
            }
            println!("{} — {} bytes — {:?}", l.title, l.size, l.paths);
        }
    }
}

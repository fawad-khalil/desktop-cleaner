//! Drive formatting (Windows and Linux only).
//!
//! Guard rails, all enforced here in the backend, not just in the UI:
//! - only volumes that pass [`eligibility`] (never the system/boot volume,
//!   the volume holding the page file, the volume this app runs from, or
//!   Linux system mount points and pseudo filesystems);
//! - the caller must pass the exact typed confirmation `FORMAT <target>`;
//! - file system and label are allowlisted/sanitised and passed as data,
//!   never spliced into a shell string;
//! - the OS elevation prompt (UAC / polkit) is always shown.

use serde::Serialize;

#[derive(Serialize)]
pub struct Volume {
    /// Drive letter mount ("E:\") on Windows, mount point on Linux.
    pub mount_point: String,
    /// Device node on Linux (/dev/sdb1); volume name on Windows.
    pub device: String,
    pub label: String,
    pub file_system: String,
    pub total: u64,
    pub removable: bool,
    /// None when the volume can be formatted, otherwise why not.
    pub blocked: Option<String>,
    /// What the user must type to confirm.
    pub confirm_text: String,
    pub file_systems: Vec<&'static str>,
}

pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));

#[cfg(windows)]
const FILE_SYSTEMS: &[&str] = &["NTFS", "exFAT", "FAT32"];
#[cfg(target_os = "linux")]
const FILE_SYSTEMS: &[&str] = &["ext4", "exfat", "vfat", "ntfs"];
#[cfg(not(any(windows, target_os = "linux")))]
const FILE_SYSTEMS: &[&str] = &[];

/// Target as the user names it: "E:" on Windows, the device on Linux.
fn target_name(mount: &str, device: &str) -> String {
    if cfg!(windows) {
        mount.trim_end_matches('\\').to_uppercase()
    } else {
        device.to_string()
    }
}

pub fn confirm_text(mount: &str, device: &str) -> String {
    format!("FORMAT {}", target_name(mount, device))
}

/// Why a volume must not be formatted, or None if it may be.
pub fn eligibility(mount: &str, device: &str, fs: &str) -> Option<String> {
    if !SUPPORTED {
        return Some("formatting is only supported on Windows and Linux".into());
    }
    let exe_mount = std::env::current_exe().ok();

    #[cfg(windows)]
    {
        let _ = (device, fs);
        let m = mount.trim_end_matches('\\').to_uppercase();
        if m.len() != 2 || !m.ends_with(':') || !m.as_bytes()[0].is_ascii_alphabetic() {
            return Some("not a lettered drive".into());
        }
        let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase();
        if m == sys {
            return Some("this is the Windows system drive".into());
        }
        if let Ok(win) = std::env::var("SystemRoot") {
            if win.to_uppercase().starts_with(&m) {
                return Some("Windows is installed on this drive".into());
            }
        }
        let root = std::path::PathBuf::from(format!("{m}\\"));
        for f in ["pagefile.sys", "swapfile.sys", "hiberfil.sys", "bootmgr"] {
            if root.join(f).exists() {
                return Some(format!("holds system file {f}"));
            }
        }
        if exe_mount.is_some_and(|e| e.to_string_lossy().to_uppercase().starts_with(&m)) {
            return Some("StorageCleaner is running from this drive".into());
        }
        None
    }

    #[cfg(target_os = "linux")]
    {
        const SYSTEM_MOUNTS: &[&str] = &[
            "/", "/boot", "/boot/efi", "/efi", "/home", "/usr", "/var", "/opt", "/srv", "/nix",
            "/tmp", "/root",
        ];
        const PSEUDO_FS: &[&str] = &[
            "tmpfs", "devtmpfs", "squashfs", "overlay", "proc", "sysfs", "swap", "ramfs",
            "efivarfs", "fuse.portal", "autofs",
        ];
        if SYSTEM_MOUNTS.contains(&mount) || mount.starts_with("/snap/") || mount.starts_with("/boot/") {
            return Some(format!("{mount} is a system mount point"));
        }
        if PSEUDO_FS.contains(&fs) {
            return Some(format!("{fs} is not a physical file system"));
        }
        if !valid_device(device) {
            return Some("not a block device".into());
        }
        if exe_mount.is_some_and(|e| e.starts_with(mount)) {
            return Some("StorageCleaner is running from this drive".into());
        }
        None
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (mount, device, fs, exe_mount);
        Some("formatting is only supported on Windows and Linux".into())
    }
}

#[cfg(target_os = "linux")]
fn valid_device(d: &str) -> bool {
    d.starts_with("/dev/")
        && d.len() > 5
        && d[5..].chars().all(|c| c.is_ascii_alphanumeric() || "_-/".contains(c))
        && !d.contains("..")
        && !d.starts_with("/dev/loop")
}

pub fn list() -> Vec<Volume> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .map(|d| {
            let mount = d.mount_point().to_string_lossy().into_owned();
            let device = d.name().to_string_lossy().into_owned();
            let fs = d.file_system().to_string_lossy().into_owned();
            Volume {
                blocked: eligibility(&mount, &device, &fs),
                confirm_text: confirm_text(&mount, &device),
                label: if cfg!(windows) { device.clone() } else { String::new() },
                mount_point: mount,
                device,
                file_system: fs,
                total: d.total_space(),
                removable: d.is_removable(),
                file_systems: FILE_SYSTEMS.to_vec(),
            }
        })
        .collect()
}

/// Keeps labels to characters every supported file system accepts.
pub fn sanitize_label(label: &str, fs: &str) -> String {
    let max = if fs.eq_ignore_ascii_case("FAT32") || fs == "vfat" { 11 } else { 32 };
    label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The exact program and arguments that would run. Exposed so the UI can
/// show it and tests can check it without running anything.
pub fn plan(mount: &str, device: &str, fs: &str, label: &str) -> anyhow::Result<(String, Vec<String>)> {
    if !FILE_SYSTEMS.contains(&fs) {
        anyhow::bail!("unsupported file system {fs}");
    }
    let label = sanitize_label(label, fs);

    #[cfg(windows)]
    {
        let _ = device;
        let letter = mount.trim_end_matches('\\').trim_end_matches(':').to_uppercase();
        if letter.len() != 1 || !letter.as_bytes()[0].is_ascii_alphabetic() {
            anyhow::bail!("invalid drive letter");
        }
        let log = std::env::temp_dir().join("storagecleaner-format.log");
        // Runs elevated. Letter, fs and label are already validated to
        // [A-Z], an allowlisted name and [A-Za-z0-9 _-].
        let inner = format!(
            "$ErrorActionPreference='Stop'; try {{ Format-Volume -DriveLetter {letter} -FileSystem {fs} \
             -NewFileSystemLabel '{label}' -Confirm:$false -Force | Out-Null; exit 0 }} \
             catch {{ $_.Exception.Message | Out-File -Encoding utf8 '{}'; exit 1 }}",
            log.display().to_string().replace('\'', "''")
        );
        let outer = format!(
            "$p = Start-Process powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
             -ArgumentList '-NoProfile','-EncodedCommand','{}'; exit $p.ExitCode",
            encode_ps(&inner)
        );
        Ok((
            "powershell.exe".into(),
            vec!["-NoProfile".into(), "-EncodedCommand".into(), encode_ps(&outer)],
        ))
    }

    #[cfg(target_os = "linux")]
    {
        let _ = mount;
        if !valid_device(device) {
            anyhow::bail!("invalid device {device}");
        }
        let (mkfs, label_flag, force) = match fs {
            "ext4" => ("mkfs.ext4", "-L", Some("-F")),
            "exfat" => ("mkfs.exfat", "-n", None),
            "vfat" => ("mkfs.vfat", "-n", None),
            "ntfs" => ("mkfs.ntfs", "-L", Some("-f")), // -f = quick format
            _ => unreachable!(),
        };
        // Values are passed as positional parameters, not interpolated.
        let script = format!(
            "set -e; if findmnt -rn -S \"$1\" >/dev/null; then umount -- \"$1\"; fi; {mkfs} {} {label_flag} \"$2\" -- \"$1\"",
            force.unwrap_or("")
        );
        Ok((
            "pkexec".into(),
            vec!["sh".into(), "-c".into(), script, "sh".into(), device.into(), label],
        ))
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (mount, device, label);
        anyhow::bail!("formatting is only supported on Windows and Linux")
    }
}

/// Formats a volume after re-checking eligibility and the typed confirmation.
pub fn format(mount: &str, fs: &str, label: &str, confirm: &str) -> anyhow::Result<()> {
    // Re-resolve the volume from the live list instead of trusting the caller.
    let vol = list()
        .into_iter()
        .find(|v| v.mount_point == mount)
        .ok_or_else(|| anyhow::anyhow!("{mount} is no longer mounted"))?;
    if let Some(why) = &vol.blocked {
        anyhow::bail!("refusing to format {mount}: {why}");
    }
    if confirm != vol.confirm_text {
        anyhow::bail!("confirmation text does not match; type exactly: {}", vol.confirm_text);
    }
    let (prog, args) = plan(&vol.mount_point, &vol.device, fs, label)?;
    let status = std::process::Command::new(prog).args(&args).status()?;
    if status.success() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        let log = std::env::temp_dir().join("storagecleaner-format.log");
        if let Ok(msg) = std::fs::read_to_string(&log) {
            let _ = std::fs::remove_file(&log);
            anyhow::bail!("format failed: {}", msg.trim());
        }
    }
    anyhow::bail!("format failed or was cancelled (exit code {:?})", status.code())
}

/// PowerShell -EncodedCommand takes base64 of UTF-16LE.
#[cfg(any(windows, test))]
fn encode_ps(script: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_utf16() {
        // "ab" in UTF-16LE = 61 00 62 00
        assert_eq!(encode_ps("ab"), "YQBiAA==");
        assert_eq!(encode_ps("a"), "YQA=");
    }

    #[test]
    fn labels_are_sanitised() {
        assert_eq!(sanitize_label("My Drive'; rm -rf /", "ext4"), "My Drive rm -rf");
        assert_eq!(sanitize_label("ABCDEFGHIJKLMNOP", "FAT32"), "ABCDEFGHIJK");
    }

    #[cfg(windows)]
    #[test]
    fn windows_system_drive_is_blocked() {
        let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        assert!(eligibility(&format!("{sys}\\"), "", "NTFS").is_some());
        assert!(eligibility("not-a-drive", "", "NTFS").is_some());
        assert_eq!(confirm_text("e:\\", ""), "FORMAT E:");
    }

    #[cfg(windows)]
    #[test]
    fn windows_plan_is_powershell_and_rejects_bad_input() {
        let (prog, args) = plan("E:\\", "", "NTFS", "Data").unwrap();
        assert_eq!(prog, "powershell.exe");
        assert_eq!(args[1], "-EncodedCommand");
        assert!(plan("E:\\", "", "ReFS; evil", "x").is_err());
        assert!(plan("EE:\\", "", "NTFS", "x").is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_rules() {
        assert!(eligibility("/", "/dev/sda2", "ext4").is_some());
        assert!(eligibility("/boot/efi", "/dev/sda1", "vfat").is_some());
        assert!(eligibility("/media/usb", "/dev/sdb1", "squashfs").is_some());
        assert!(eligibility("/media/usb", "/dev/../sda", "ext4").is_some());
        assert!(eligibility("/media/usb", "/dev/sdb1", "vfat").is_none());
        let (prog, args) = plan("/media/usb", "/dev/sdb1", "ext4", "Data; reboot").unwrap();
        assert_eq!(prog, "pkexec");
        assert_eq!(args[4], "/dev/sdb1");
        assert_eq!(args[5], "Data reboot");
    }

    #[test]
    fn listing_never_offers_the_system_volume() {
        for v in list() {
            #[cfg(windows)]
            if v.mount_point.to_uppercase().starts_with(&std::env::var("SystemDrive").unwrap_or("C:".into())) {
                assert!(v.blocked.is_some());
            }
            #[cfg(unix)]
            if v.mount_point == "/" {
                assert!(v.blocked.is_some());
            }
        }
    }
}


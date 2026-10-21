//! macOS privacy (TCC) awareness: whether the app has Full Disk Access, and
//! which folders to leave alone without it.
//!
//! Reading a folder macOS protects either shows a permission prompt
//! (Contacts, Calendars, Photos, other apps' data, iCloud Drive…) or fails
//! silently (Mail, Messages, Safari). Ad-hoc-signed builds get a new code
//! signature every build, so macOS forgets the answers and asks again. With
//! Full Disk Access none of this applies; without it, scans skip these
//! folders entirely and count them as skipped ("System & other").
//!
//! Desktop, Documents and Downloads are not skipped: they are the user's own
//! files and the point of a scan. macOS asks for them once per signature.
//!
//! Other platforms have no such protection: everything here is a no-op.

use serde::Serialize;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU8, Ordering};

/// 0 = not checked yet, 1 = granted, 2 = not granted.
#[cfg(target_os = "macos")]
static FDA: AtomicU8 = AtomicU8::new(0);

/// Checks Full Disk Access without prompting: files that only FDA can read
/// fail with "operation not permitted" instead of asking. Several probes,
/// since some macOS versions keep TCC.db closed even with access: any one
/// that reads means granted.
#[cfg(target_os = "macos")]
fn probe() -> bool {
    let Some(home) = dirs::home_dir() else { return true };
    let probes = [
        home.join("Library/Application Support/com.apple.TCC/TCC.db"),
        PathBuf::from("/Library/Application Support/com.apple.TCC/TCC.db"),
        home.join("Library/Safari/Bookmarks.plist"),
        home.join("Library/Safari"),
        home.join("Library/Mail"),
        home.join("Library/Messages"),
    ];
    let results: Vec<(&PathBuf, Option<std::io::ErrorKind>)> = probes
        .iter()
        .filter_map(|p| {
            let r = if p.is_dir() { std::fs::read_dir(p).map(|_| ()) } else { std::fs::File::open(p).map(|_| ()) };
            match r {
                Ok(()) => Some((p, None)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => Some((p, Some(e.kind()))),
            }
        })
        .collect();
    for (p, r) in &results {
        eprintln!("full disk access probe {}: {}", p.display(), r.map_or("readable".to_string(), |k| format!("{k:?}")));
    }
    granted_from(&results.iter().map(|(_, r)| *r).collect::<Vec<_>>())
}

/// Any readable probe means access; otherwise any refusal means none. With
/// nothing to tell from (no probe exists), assume it's fine.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn granted_from(results: &[Option<std::io::ErrorKind>]) -> bool {
    results.iter().any(Option::is_none) || !results.iter().any(|r| r == &Some(std::io::ErrorKind::PermissionDenied))
}

/// Whether the app has Full Disk Access (always true off macOS). Re-checks
/// when `recheck`, e.g. after the user has been to System Settings.
pub fn full_disk_access(recheck: bool) -> bool {
    #[cfg(target_os = "macos")]
    {
        let v = FDA.load(Ordering::Relaxed);
        if v != 0 && !recheck {
            return v == 1;
        }
        let ok = probe();
        FDA.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
        ok
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = recheck;
        true
    }
}

/// What the Full Disk Access card needs to know.
#[derive(Serialize)]
pub struct Access {
    pub granted: bool,
    /// Not running as an app bundle (e.g. `npm run tauri dev`): macOS
    /// applies the permission of the app that launched it, named here.
    pub host: Option<String>,
    /// The app's signature isn't sealed to its bundle (linker-signed only),
    /// so a grant in System Settings can't be matched to it.
    pub unsealed: bool,
}

/// The `.app` bundle an executable belongs to (`X.app/Contents/MacOS/x`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && app.extension()? == "app").then(|| app.to_path_buf())
}

/// Name of the first app in a chain of parent executables (nearest first),
/// e.g. "Terminal" or "Visual Studio Code" for helpers inside their bundles.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn host_app(parents: &[PathBuf]) -> Option<String> {
    parents.iter().find_map(|exe| {
        // Helpers live in nested bundles (Code Helper.app inside Code.app):
        // take the outermost .app on the path.
        let outer = exe.ancestors().filter(|a| a.extension().is_some_and(|e| e == "app")).last()?;
        Some(outer.file_stem()?.to_string_lossy().into_owned())
    })
}

/// Whether `codesign -dv` output shows a signature macOS can't tie to the
/// bundle: only the linker's (identifier isn't the bundle id) or no
/// Info.plist bound.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn unsealed(codesign: &str, bundle_id: &str) -> bool {
    let id = codesign.lines().find_map(|l| l.strip_prefix("Identifier="));
    id != Some(bundle_id) || codesign.contains("Info.plist=not bound") || codesign.contains("linker-signed")
}

/// Full Disk Access plus why it might not apply to this process.
pub fn access(recheck: bool) -> Access {
    let granted = full_disk_access(recheck);
    #[cfg(target_os = "macos")]
    if !granted {
        let exe = std::env::current_exe().unwrap_or_default();
        if let Some(app) = bundle_of(&exe) {
            let out = std::process::Command::new("/usr/bin/codesign").arg("-dv").arg(&app).output();
            let text = out.map(|o| String::from_utf8_lossy(&o.stderr).into_owned()).unwrap_or_default();
            let id = crate::uninstall::bundle_id(&app).unwrap_or_default();
            return Access { granted, host: None, unsealed: unsealed(&text, &id) };
        }
        return Access { granted, host: launcher(), unsealed: false };
    }
    Access { granted, host: None, unsealed: false }
}

/// The app that launched this process, for builds not run as a bundle.
#[cfg(target_os = "macos")]
fn launcher() -> Option<String> {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut parents = Vec::new();
    let mut cur = sysinfo::get_current_pid().ok().and_then(|p| sys.process(p)).and_then(|p| p.parent());
    while let Some(pid) = cur {
        let Some(p) = sys.process(pid) else { break };
        parents.extend(p.exe().map(Path::to_path_buf));
        cur = p.parent().filter(|&x| x != pid);
    }
    host_app(&parents)
}

/// Folders under the home folder that macOS guards with a prompt or with
/// Full Disk Access.
#[cfg(target_os = "macos")]
const HOME_PROTECTED: [&str; 24] = [
    "Library/Mail",
    "Library/Messages",
    "Library/Safari",
    "Library/Caches/com.apple.Safari",
    "Library/Containers",
    "Library/Group Containers",
    "Library/Application Support/AddressBook",
    "Library/Application Support/CallHistoryDB",
    "Library/Application Support/CallHistoryTransactions",
    "Library/Application Support/com.apple.TCC",
    "Library/Application Support/FaceTime",
    "Library/Application Support/Knowledge",
    "Library/Application Support/MobileSync",
    "Library/Calendars",
    "Library/Reminders",
    "Library/Cookies",
    "Library/HomeKit",
    "Library/IdentityServices",
    "Library/Metadata/CoreSpotlight",
    "Library/Suggestions",
    "Library/Mobile Documents",
    "Library/CloudStorage",
    "Library/Photos",
    ".Trash",
];

/// Whether `path` is a folder macOS protects (on any Mac, granted or not).
#[cfg(target_os = "macos")]
pub fn tcc_protected(path: &Path) -> bool {
    // Photos, Photo Booth and other media libraries are bundles anywhere.
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("photoslibrary") || e.eq_ignore_ascii_case("photolibrary")) {
        return true;
    }
    let Some(home) = dirs::home_dir() else { return false };
    let Ok(rel) = path.strip_prefix(&home) else { return false };
    HOME_PROTECTED.iter().any(|p| rel == Path::new(p))
}

/// True when `path` must not be read: it's protected and the app doesn't
/// have Full Disk Access. Checked before every folder is read.
pub fn withheld(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        tcc_protected(path) && !full_disk_access(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

#[cfg(test)]
mod pure_tests {
    use super::*;
    use std::io::ErrorKind::{NotFound, PermissionDenied};

    #[test]
    fn any_readable_probe_means_granted() {
        assert!(granted_from(&[Some(PermissionDenied), None]));
        assert!(!granted_from(&[Some(PermissionDenied), Some(PermissionDenied)]));
        assert!(granted_from(&[]));
        assert!(granted_from(&[Some(NotFound)]));
    }

    #[test]
    fn finds_bundles_and_launching_apps() {
        let p = PathBuf::from;
        assert_eq!(bundle_of(&p("/Applications/DesktopCleaner.app/Contents/MacOS/desktop-cleaner")), Some(p("/Applications/DesktopCleaner.app")));
        assert_eq!(bundle_of(&p("/Users/me/src/target/debug/desktop-cleaner")), None);
        let chain = [
            p("/opt/homebrew/bin/node"),
            p("/bin/zsh"),
            p("/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)"),
            p("/Applications/Visual Studio Code.app/Contents/MacOS/Electron"),
        ];
        assert_eq!(host_app(&chain).as_deref(), Some("Visual Studio Code"));
        assert_eq!(host_app(&[p("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal")]).as_deref(), Some("Terminal"));
        assert_eq!(host_app(&[p("/sbin/launchd")]), None);
    }

    #[test]
    fn spots_signatures_not_sealed_to_the_bundle() {
        let id = "com.fawadkhalil.desktopcleaner";
        let linker = "Identifier=desktop_cleaner-ff2c692c2f470efb\nCodeDirectory v=20400 flags=0x20002(adhoc,linker-signed)\nInfo.plist=not bound\n";
        assert!(unsealed(linker, id));
        let adhoc = "Identifier=com.fawadkhalil.desktopcleaner\nCodeDirectory v=20500 flags=0x10002(adhoc,runtime)\nInfo.plist entries=15\n";
        assert!(!unsealed(adhoc, id));
        assert!(unsealed("code object is not signed at all", id));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn knows_protected_folders() {
        let home = dirs::home_dir().unwrap();
        assert!(tcc_protected(&home.join("Library/Containers")));
        assert!(tcc_protected(&home.join("Library/Group Containers")));
        assert!(tcc_protected(&home.join("Library/Application Support/AddressBook")));
        assert!(tcc_protected(&home.join("Pictures/Photos Library.photoslibrary")));
        // Only the protected folder itself: scans never get below it.
        assert!(!tcc_protected(&home.join("Library")));
        assert!(!tcc_protected(&home.join("Library/Caches")));
        assert!(!tcc_protected(&home.join("Documents")));
        assert!(!tcc_protected(Path::new("/Library/Containers")));
    }

    #[test]
    fn probe_does_not_panic() {
        let granted = full_disk_access(true);
        assert_eq!(withheld(&dirs::home_dir().unwrap().join("Library/Mail")), !granted);
    }
}

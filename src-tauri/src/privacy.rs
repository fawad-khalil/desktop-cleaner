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

use std::path::Path;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU8, Ordering};

/// 0 = not checked yet, 1 = granted, 2 = not granted.
#[cfg(target_os = "macos")]
static FDA: AtomicU8 = AtomicU8::new(0);

/// Checks Full Disk Access without prompting: files that only FDA can read
/// fail with "operation not permitted" instead of asking.
#[cfg(target_os = "macos")]
fn probe() -> bool {
    let Some(home) = dirs::home_dir() else { return true };
    let probes = [
        home.join("Library/Application Support/com.apple.TCC/TCC.db"),
        home.join("Library/Safari"),
        home.join("Library/Mail"),
    ];
    for p in &probes {
        let r = if p.is_dir() { std::fs::read_dir(p).map(|_| ()) } else { std::fs::File::open(p).map(|_| ()) };
        match r {
            Ok(()) => return true,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return false,
            Err(_) => {}
        }
    }
    // None of them exist: nothing to tell from, assume it's fine.
    true
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

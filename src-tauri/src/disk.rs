//! Drive listing and directory size scanning.

use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::Disks;

#[derive(Serialize)]
pub struct DriveInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub kind: String,
    pub total: u64,
    pub available: u64,
    pub removable: bool,
}

pub fn list_drives() -> Vec<DriveInfo> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        // On macOS the data volume (and VM, Preboot…) share the boot
        // volume's APFS container; "/" already shows the whole container.
        .filter(|d| !(cfg!(target_os = "macos") && d.mount_point().starts_with("/System/Volumes")))
        .map(|d| DriveInfo {
            name: d.name().to_string_lossy().into_owned(),
            mount_point: d.mount_point().to_string_lossy().into_owned(),
            file_system: d.file_system().to_string_lossy().into_owned(),
            kind: format!("{:?}", d.kind()),
            total: d.total_space(),
            available: d.available_space(),
            removable: d.is_removable(),
        })
        .collect()
}

#[derive(Serialize, Clone)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub files: u64,
    /// The largest items one level further down (only filled for the
    /// direct children of a scan, so the UI can draw a second ring).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Entry>,
}

/// Files seen by the scan in progress, so the UI can show it's moving.
static SCAN_PROGRESS: AtomicU64 = AtomicU64::new(0);

pub fn scan_progress() -> u64 {
    SCAN_PROGRESS.load(Ordering::Relaxed)
}

pub(crate) fn bump_progress(reset: bool) {
    if reset {
        SCAN_PROGRESS.store(0, Ordering::Relaxed);
    } else {
        SCAN_PROGRESS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Remembers which device a scan starts on so other mounts are skipped
/// (Linux only; macOS skips known paths instead).
pub(crate) fn set_scan_root(md: &fs::Metadata) {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::os::unix::fs::MetadataExt;
        SCAN_DEV.store(md.dev(), Ordering::Relaxed);
    }
    let _ = md;
}

/// Directories that are never descended into: other mounts and virtual
/// file systems that would double-count or never end. `root` itself is
/// always scanned so the user can still pick one explicitly.
pub(crate) fn skip_dir(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        // /System/Volumes/Data holds the same files as /Users, /Applications
        // etc. through firmlinks; /Volumes holds other drives, /Network and
        // /net network shares (reading those asks for permission). A drive
        // the user opens itself is a scan root, so it is still read.
        if ["/System/Volumes", "/Volumes", "/Network", "/net", "/dev"].iter().any(|p| path == Path::new(p)) {
            return true;
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if ["/proc", "/sys", "/dev", "/run"].iter().any(|p| path == Path::new(p)) {
            return true;
        }
    }
    let _ = path;
    false
}

#[derive(Serialize, Clone)]
pub struct TypeBucket {
    pub category: String,
    pub size: u64,
    pub files: u64,
}

#[derive(Serialize)]
pub struct ScanResult {
    pub path: String,
    pub total: u64,
    pub files: u64,
    pub entries: Vec<Entry>,
    pub types: Vec<TypeBucket>,
    /// Size and count per last-used bucket, indexed like `AGE_LABELS`.
    pub ages: Vec<TypeBucket>,
    /// Number of directories that could not be read (permissions etc.).
    pub skipped: u64,
    /// The folder the cached index was scanned from (e.g. the drive).
    pub root: String,
    /// When the index was last brought up to date / fully rescanned
    /// (seconds since the epoch).
    pub updated_at: u64,
    pub full_scan_at: u64,
}

#[derive(Default)]
struct Acc {
    size: u64,
    files: u64,
    skipped: u64,
    types: HashMap<&'static str, (u64, u64)>,
    ages: [(u64, u64); 5],
    /// Most recent last-used time of any file, seconds since the epoch.
    newest: u64,
}

pub const AGE_LABELS: [&str; 5] = [
    "Used in the last month",
    "1-6 months ago",
    "6-12 months ago",
    "1-2 years ago",
    "Over 2 years ago",
];

/// Bucket index for a file's last use (later of access and modification).
pub fn age_bucket(md: &fs::Metadata) -> usize {
    let used = crate::cleanup::last_used(md).unwrap_or(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = now.saturating_sub(used) / 86_400;
    match days {
        0..30 => 0,
        30..182 => 1,
        182..365 => 2,
        365..730 => 3,
        _ => 4,
    }
}

impl Acc {
    fn merge(mut self, other: Acc) -> Acc {
        self.size += other.size;
        self.files += other.files;
        self.skipped += other.skipped;
        self.newest = self.newest.max(other.newest);
        for (k, (s, f)) in other.types {
            let e = self.types.entry(k).or_default();
            e.0 += s;
            e.1 += f;
        }
        for (a, b) in self.ages.iter_mut().zip(other.ages) {
            a.0 += b.0;
            a.1 += b.1;
        }
        self
    }

    fn add_file(&mut self, path: &Path, md: &fs::Metadata) {
        SCAN_PROGRESS.fetch_add(1, Ordering::Relaxed);
        self.newest = self.newest.max(crate::cleanup::last_used(md).unwrap_or(0));
        let len = md.len();
        let a = &mut self.ages[age_bucket(md)];
        a.0 += len;
        a.1 += 1;
        self.size += len;
        self.files += 1;
        let e = self.types.entry(category_for(path)).or_default();
        e.0 += len;
        e.1 += 1;
    }
}

/// Recursively sizes `dir`. Symlinks and junctions are never followed, so a
/// scan can't loop or escape into another volume.
fn walk(dir: &Path) -> Acc {
    // Protected without Full Disk Access: not read, so no prompt; counted
    // as skipped like any folder that can't be read.
    let rd = match (!crate::privacy::withheld(dir)).then(|| fs::read_dir(dir)) {
        Some(Ok(rd)) => rd,
        _ => {
            return Acc {
                skipped: 1,
                ..Default::default()
            };
        }
    };
    let mut subdirs = Vec::new();
    let mut acc = Acc::default();
    for ent in rd.flatten() {
        let Ok(ft) = ent.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            let p = ent.path();
            if !skip_dir(&p) && !other_device(&p) {
                subdirs.push(p);
            }
        } else if ft.is_file() {
            if let Ok(md) = ent.metadata() {
                acc.add_file(&ent.path(), &md);
            }
        }
    }
    let sub = subdirs
        .par_iter()
        .map(|d| walk(d))
        .reduce(Acc::default, Acc::merge);
    acc.merge(sub)
}

/// On Linux, a directory on another device is a separate mount (a USB
/// drive under /media, a network share…) and belongs to another drive.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn other_device(p: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::symlink_metadata(p), SCAN_DEV.load(Ordering::Relaxed)) {
        (Ok(md), dev) if dev != 0 => md.dev() != dev,
        _ => false,
    }
}
#[cfg(not(all(unix, not(target_os = "macos"))))]
pub(crate) fn other_device(_: &Path) -> bool {
    false
}

/// Device of the current scan root (Linux only; 0 = unknown).
#[cfg(all(unix, not(target_os = "macos")))]
static SCAN_DEV: AtomicU64 = AtomicU64::new(0);

pub fn category_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "tif" | "tiff" | "svg"
        | "raw" | "cr2" | "nef" | "psd" | "ico" => "Images",
        "mp4" | "mkv" | "mov" | "avi" | "wmv" | "webm" | "m4v" | "flv" => "Videos",
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" => "Audio",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "txt"
        | "md" | "rtf" | "csv" | "epub" => "Documents",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" => "Archives",
        "iso" | "img" | "dmg" | "vhd" | "vhdx" | "vmdk" | "qcow2" | "wim" => "Disk images",
        "exe" | "msi" | "dll" | "sys" | "so" | "dylib" | "app" | "deb" | "rpm" | "appimage"
        | "pkg" | "msix" | "appx" => "Applications & binaries",
        "rs" | "js" | "ts" | "py" | "c" | "cpp" | "h" | "java" | "go" | "cs" | "html" | "css"
        | "json" | "xml" | "yaml" | "yml" | "toml" | "sh" | "ps1" | "svelte" => "Code",
        "log" | "tmp" | "temp" | "bak" | "old" | "dmp" | "etl" => "Logs & temp",
        "db" | "sqlite" | "mdb" | "ldf" | "mdf" | "pack" | "idx" => "Databases",
        _ => "Other",
    }
}

/// Total size and file count of a directory tree (symlinks not followed).
pub fn dir_size(path: &Path) -> (u64, u64) {
    let acc = walk(path);
    (acc.size, acc.files)
}

/// Size of a directory tree and the last time any file in it was used
/// (`None` when it holds no files).
pub fn dir_size_and_newest(path: &Path) -> (u64, Option<u64>) {
    let acc = walk(path);
    (acc.size, (acc.newest > 0).then_some(acc.newest))
}

#[derive(Serialize)]
pub struct FileItem {
    pub path: String,
    pub size: u64,
    pub last_used: Option<u64>,
}

/// The `limit` largest files under `path`, optionally filtered by type
/// category, last-used bucket and/or a name/path search.
pub fn largest_files(
    path: &str,
    category: Option<&str>,
    age: Option<usize>,
    limit: usize,
    search: Option<&crate::search::Matcher>,
) -> Vec<FileItem> {
    fn go(dir: &Path, category: Option<&str>, age: Option<usize>, limit: usize, search: Option<&crate::search::Matcher>) -> Vec<FileItem> {
        if crate::privacy::withheld(dir) {
            return Vec::new();
        }
        let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
        let mut subdirs = Vec::new();
        let mut out = Vec::new();
        for ent in rd.flatten() {
            let Ok(ft) = ent.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let p = ent.path();
            if ft.is_dir() {
                // Like the scan, other drives (removable, network) and the
                // data volume's second copy of the disk aren't read.
                if !skip_dir(&p) {
                    subdirs.push(p);
                }
            } else if ft.is_file() {
                if category.is_some_and(|c| c != category_for(&p)) {
                    continue;
                }
                if search.is_some_and(|m| !m.is_match(&p.to_string_lossy())) {
                    continue;
                }
                let Ok(md) = ent.metadata() else { continue };
                if age.is_some_and(|a| a != age_bucket(&md)) {
                    continue;
                }
                out.push(FileItem {
                    path: p.to_string_lossy().into_owned(),
                    size: md.len(),
                    last_used: crate::cleanup::last_used(&md),
                });
            }
        }
        let mut all = subdirs
            .par_iter()
            .map(|d| go(d, category, age, limit, search))
            .reduce(Vec::new, |mut a, b| {
                a.extend(b);
                a
            });
        all.extend(out);
        all.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        all.truncate(limit);
        all
    }
    go(Path::new(path), category, age, limit, search)
}

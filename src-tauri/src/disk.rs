//! Drive listing and directory size scanning.

use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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

/// How many grandchildren are kept per child; the rest show as a gap.
const MAX_GRANDCHILDREN: usize = 24;

/// Files seen by the scan in progress, so the UI can show it's moving.
static SCAN_PROGRESS: AtomicU64 = AtomicU64::new(0);

pub fn scan_progress() -> u64 {
    SCAN_PROGRESS.load(Ordering::Relaxed)
}

/// Directories that are never descended into: other mounts and virtual
/// file systems that would double-count or never end. `root` itself is
/// always scanned so the user can still pick one explicitly.
fn skip_dir(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        // /System/Volumes/Data holds the same files as /Users, /Applications
        // etc. through firmlinks; /Volumes holds other drives.
        if path == Path::new("/System/Volumes") || path == Path::new("/Volumes") || path == Path::new("/dev") {
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
}

#[derive(Default)]
struct Acc {
    size: u64,
    files: u64,
    skipped: u64,
    types: HashMap<&'static str, (u64, u64)>,
    ages: [(u64, u64); 5],
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
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => {
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
fn other_device(p: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::symlink_metadata(p), SCAN_DEV.load(Ordering::Relaxed)) {
        (Ok(md), dev) if dev != 0 => md.dev() != dev,
        _ => false,
    }
}
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn other_device(_: &Path) -> bool {
    false
}

/// Device of the current scan root (Linux only; 0 = unknown).
static SCAN_DEV: AtomicU64 = AtomicU64::new(0);

/// A directory (or large file) remembered from the last scan, so moving
/// around the tree doesn't need another pass over the disk. Items smaller
/// than `KEEP_MIN` are only counted in their parent's totals.
struct Node {
    name: Box<str>,
    size: u64,
    files: u64,
    is_dir: bool,
    skipped: u64,
    /// Largest first.
    children: Vec<Node>,
    stats: Option<Box<Stats>>,
}

struct Stats {
    types: Vec<TypeBucket>,
    ages: Vec<TypeBucket>,
}

const KEEP_MIN: u64 = 1 << 20;
const MAX_FILES_KEPT: usize = 64;
/// Type/age breakdowns are kept for big folders; small ones are re-walked
/// on demand, which is quick.
const STATS_MIN: u64 = 64 << 20;

/// Recently scanned trees and the paths they were scanned from (e.g. one
/// per drive), newest last.
static TREES: Mutex<Vec<(PathBuf, Node)>> = Mutex::new(Vec::new());
const MAX_TREES: usize = 4;

fn stats(acc: &Acc) -> Stats {
    let mut types: Vec<TypeBucket> = acc
        .types
        .iter()
        .map(|(k, (size, files))| TypeBucket { category: k.to_string(), size: *size, files: *files })
        .collect();
    types.sort_by(|a, b| b.size.cmp(&a.size));
    let ages = AGE_LABELS
        .iter()
        .zip(acc.ages)
        .map(|(l, (size, files))| TypeBucket { category: l.to_string(), size, files })
        .collect();
    Stats { types, ages }
}

/// Like `walk`, but also builds the `Node` tree.
fn walk_tree(dir: &Path, name: Box<str>) -> (Acc, Node) {
    let mut node = Node { name, size: 0, files: 0, is_dir: true, skipped: 0, children: Vec::new(), stats: None };
    let Ok(rd) = fs::read_dir(dir) else {
        node.skipped = 1;
        return (Acc { skipped: 1, ..Default::default() }, node);
    };
    let mut subdirs = Vec::new();
    let mut acc = Acc::default();
    let mut big_files = Vec::new();
    for ent in rd.flatten() {
        let Ok(ft) = ent.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let p = ent.path();
        if ft.is_dir() {
            if !skip_dir(&p) && !other_device(&p) {
                subdirs.push(p);
            }
        } else if ft.is_file() {
            let Ok(md) = ent.metadata() else { continue };
            acc.add_file(&p, &md);
            if md.len() >= KEEP_MIN {
                big_files.push(Node {
                    name: ent.file_name().to_string_lossy().into(),
                    size: md.len(),
                    files: 1,
                    is_dir: false,
                    skipped: 0,
                    children: Vec::new(),
                    stats: None,
                });
            }
        }
    }
    let subs: Vec<(Acc, Node)> = subdirs
        .par_iter()
        .map(|d| {
            let name = d.file_name().map(|n| n.to_string_lossy().into()).unwrap_or_default();
            walk_tree(d, name)
        })
        .collect();
    big_files.sort_by(|a, b| b.size.cmp(&a.size));
    big_files.truncate(MAX_FILES_KEPT);
    let mut children = big_files;
    for (a, n) in subs {
        acc = acc.merge(a);
        if n.size >= KEEP_MIN {
            children.push(n);
        }
    }
    children.sort_by(|a, b| b.size.cmp(&a.size));
    node.children = children;
    node.size = acc.size;
    node.files = acc.files;
    node.skipped = acc.skipped;
    if acc.size >= STATS_MIN {
        node.stats = Some(Box::new(stats(&acc)));
    }
    (acc, node)
}

fn result_from(path: &Path, node: &Node) -> ScanResult {
    let entry = |n: &Node, parent: &Path, kids: Vec<Entry>| Entry {
        name: n.name.to_string(),
        path: parent.join(&*n.name).to_string_lossy().into_owned(),
        size: n.size,
        is_dir: n.is_dir,
        files: n.files,
        children: kids,
    };
    let entries = node
        .children
        .iter()
        .map(|c| {
            let cp = path.join(&*c.name);
            let kids = c.children.iter().take(MAX_GRANDCHILDREN).map(|g| entry(g, &cp, Vec::new())).collect();
            entry(c, path, kids)
        })
        .collect();
    let (types, ages) = match &node.stats {
        Some(s) => (s.types.clone(), s.ages.clone()),
        None => (Vec::new(), Vec::new()),
    };
    ScanResult {
        path: path.to_string_lossy().into_owned(),
        total: node.size,
        files: node.files,
        entries,
        types,
        ages,
        skipped: node.skipped,
    }
}

/// Finds `path` in the cached tree.
fn cached(path: &Path) -> Option<ScanResult> {
    let mut res = {
        let g = TREES.lock().ok()?;
        g.iter().rev().find_map(|(root, tree)| {
            let rel = path.strip_prefix(root).ok()?;
            let mut node = tree;
            for c in rel.components() {
                let name = c.as_os_str().to_string_lossy();
                node = node.children.iter().find(|n| n.is_dir && *n.name == *name)?;
            }
            Some(result_from(path, node))
        })?
    };
    if res.types.is_empty() && res.total > 0 {
        let s = stats(&walk(path));
        res.types = s.types;
        res.ages = s.ages;
    }
    Some(res)
}

/// Puts a freshly scanned subtree into the cached tree, fixing up the
/// totals of its ancestors. Returns the size/file deltas applied.
fn graft(node: &mut Node, comps: &[String], new: Node) -> Option<(i64, i64)> {
    let (first, rest) = comps.split_first()?;
    let i = node.children.iter().position(|c| c.is_dir && *c.name == **first);
    let d = if rest.is_empty() {
        let (os, of) = i.map(|i| (node.children[i].size, node.children[i].files)).unwrap_or((0, 0));
        let d = (new.size as i64 - os as i64, new.files as i64 - of as i64);
        match i {
            Some(i) => node.children[i] = new,
            None => node.children.push(new),
        }
        d
    } else {
        graft(&mut node.children[i?], rest, new)?
    };
    node.size = (node.size as i64 + d.0).max(0) as u64;
    node.files = (node.files as i64 + d.1).max(0) as u64;
    node.children.sort_by(|a, b| b.size.cmp(&a.size));
    Some(d)
}

/// Scans `path` and returns its direct children (each with its own largest
/// children for a second ring) plus type and last-used breakdowns of the
/// whole tree. Unless `fresh`, a folder inside the last scan is answered
/// from memory.
pub fn scan(path: &str, fresh: bool) -> anyhow::Result<ScanResult> {
    let root = PathBuf::from(path);
    if !fresh {
        if let Some(r) = cached(&root) {
            return Ok(r);
        }
    }
    let md = fs::symlink_metadata(&root)?;
    if !md.is_dir() {
        anyhow::bail!("{path} is not a directory");
    }
    SCAN_PROGRESS.store(0, Ordering::Relaxed);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        SCAN_DEV.store(if cfg!(target_os = "macos") { 0 } else { md.dev() }, Ordering::Relaxed);
    }

    let name: Box<str> = root.file_name().map(|n| n.to_string_lossy().into()).unwrap_or_default();
    let (acc, mut node) = walk_tree(&root, name);
    if node.stats.is_none() {
        node.stats = Some(Box::new(stats(&acc)));
    }
    let res = result_from(&root, &node);

    let mut g = TREES.lock().unwrap_or_else(|e| e.into_inner());
    let grafted = g.iter_mut().any(|(r, tree)| {
        let Ok(rel) = root.strip_prefix(&*r) else { return false };
        let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        // `node` is only moved in when the path is found.
        !comps.is_empty() && graft_checked(tree, &comps, &mut node)
    });
    if !grafted {
        // A new root replaces any trees it contains.
        g.retain(|(r, _)| !r.starts_with(&root));
        g.push((root, node));
        if g.len() > MAX_TREES {
            g.remove(0);
        }
    }
    Ok(res)
}

/// `graft`, but leaves `new` in place when the path isn't in the tree.
fn graft_checked(tree: &mut Node, comps: &[String], new: &mut Node) -> bool {
    // Check the parent exists first so `new` isn't consumed on a miss.
    let mut n = &*tree;
    for c in &comps[..comps.len() - 1] {
        match n.children.iter().find(|x| x.is_dir && *x.name == **c) {
            Some(x) => n = x,
            None => return false,
        }
    }
    let taken = std::mem::replace(
        new,
        Node { name: "".into(), size: 0, files: 0, is_dir: true, skipped: 0, children: Vec::new(), stats: None },
    );
    graft(tree, comps, taken).is_some()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_sizes_children_and_types() {
        const M: usize = 1 << 20;
        let root = std::env::temp_dir().join(format!("sc-scan-test-{}", std::process::id()));
        fs::create_dir_all(root.join("sub/deeper")).unwrap();
        fs::write(root.join("a.txt"), vec![0u8; M]).unwrap();
        fs::write(root.join("sub/b.png"), vec![0u8; 3 * M]).unwrap();
        fs::write(root.join("sub/deeper/c.png"), vec![0u8; 6 * M]).unwrap();
        fs::write(root.join("sub/deeper/tiny.txt"), vec![0u8; 10]).unwrap();
        let mb = |n: usize| (n * M) as u64;

        let r = scan(root.to_str().unwrap(), true).unwrap();
        assert_eq!(r.total, mb(10) + 10);
        assert_eq!(r.files, 4);
        assert_eq!(r.entries[0].name, "sub");
        assert_eq!(r.entries[0].size, mb(9) + 10);
        let kids: Vec<_> = r.entries[0].children.iter().map(|e| (e.name.as_str(), e.size)).collect();
        assert_eq!(kids, [("deeper", mb(6) + 10), ("b.png", mb(3))]);
        assert_eq!(r.ages[0].files, 4); // all just written
        let big = largest_files(root.to_str().unwrap(), Some("Images"), None, 1);
        assert_eq!(big.len(), 1);
        assert_eq!(big[0].size, mb(6));
        let images = r.types.iter().find(|t| t.category == "Images").unwrap();
        assert_eq!((images.size, images.files), (mb(9), 2));

        // Drilling down is answered from the cached tree, including the
        // small file that isn't listed but still counts.
        let deeper = root.join("sub/deeper");
        let d = cached(&deeper).expect("subfolder should be cached");
        assert_eq!(d.total, mb(6) + 10);
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.types.iter().map(|t| t.files).sum::<u64>(), 2);

        // A fresh scan of a subfolder is grafted in and fixes the totals above it.
        fs::write(deeper.join("d.png"), vec![0u8; 2 * M]).unwrap();
        scan(deeper.to_str().unwrap(), true).unwrap();
        let top = cached(&root).unwrap();
        assert_eq!(top.total, mb(12) + 10);

        fs::remove_dir_all(&root).unwrap();
    }
}

#[derive(Serialize)]
pub struct FileItem {
    pub path: String,
    pub size: u64,
    pub last_used: Option<u64>,
}

/// The `limit` largest files under `path`, optionally filtered by type
/// category and/or last-used bucket.
pub fn largest_files(path: &str, category: Option<&str>, age: Option<usize>, limit: usize) -> Vec<FileItem> {
    fn go(dir: &Path, category: Option<&str>, age: Option<usize>, limit: usize) -> Vec<FileItem> {
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
                subdirs.push(p);
            } else if ft.is_file() {
                if category.is_some_and(|c| c != category_for(&p)) {
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
            .map(|d| go(d, category, age, limit))
            .reduce(Vec::new, |mut a, b| {
                a.extend(b);
                a
            });
        all.extend(out);
        all.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        all.truncate(limit);
        all
    }
    go(Path::new(path), category, age, limit)
}

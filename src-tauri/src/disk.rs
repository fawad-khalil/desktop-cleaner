//! Drive listing and directory size scanning.

use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
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
            subdirs.push(ent.path());
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

/// Scans one level of `path`: every direct child gets its recursive size,
/// and the whole tree is bucketed by file type.
pub fn scan(path: &str) -> anyhow::Result<ScanResult> {
    let root = PathBuf::from(path);
    let md = fs::symlink_metadata(&root)?;
    if !md.is_dir() {
        anyhow::bail!("{path} is not a directory");
    }

    let children: Vec<_> = fs::read_dir(&root)?.flatten().collect();
    let results: Vec<(Entry, Acc)> = children
        .par_iter()
        .filter_map(|ent| {
            let ft = ent.file_type().ok()?;
            if ft.is_symlink() {
                return None;
            }
            let p = ent.path();
            let name = ent.file_name().to_string_lossy().into_owned();
            let acc = if ft.is_dir() {
                walk(&p)
            } else {
                let mut a = Acc::default();
                a.add_file(&p, &ent.metadata().ok()?);
                a
            };
            Some((
                Entry {
                    name,
                    path: p.to_string_lossy().into_owned(),
                    size: acc.size,
                    is_dir: ft.is_dir(),
                    files: acc.files,
                },
                acc,
            ))
        })
        .collect();

    let mut entries = Vec::with_capacity(results.len());
    let mut total = Acc::default();
    for (e, a) in results {
        entries.push(e);
        total = total.merge(a);
    }
    entries.sort_by(|a, b| b.size.cmp(&a.size));

    let mut types: Vec<TypeBucket> = total
        .types
        .into_iter()
        .map(|(k, (size, files))| TypeBucket {
            category: k.to_string(),
            size,
            files,
        })
        .collect();
    types.sort_by(|a, b| b.size.cmp(&a.size));

    let ages = AGE_LABELS
        .iter()
        .zip(total.ages)
        .map(|(l, (size, files))| TypeBucket {
            category: l.to_string(),
            size,
            files,
        })
        .collect();

    Ok(ScanResult {
        ages,
        path: root.to_string_lossy().into_owned(),
        total: total.size,
        files: total.files,
        entries,
        types,
        skipped: total.skipped,
    })
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
        let root = std::env::temp_dir().join(format!("sc-scan-test-{}", std::process::id()));
        fs::create_dir_all(root.join("sub/deeper")).unwrap();
        fs::write(root.join("a.txt"), vec![0u8; 100]).unwrap();
        fs::write(root.join("sub/b.png"), vec![0u8; 300]).unwrap();
        fs::write(root.join("sub/deeper/c.png"), vec![0u8; 600]).unwrap();

        let r = scan(root.to_str().unwrap()).unwrap();
        assert_eq!(r.total, 1000);
        assert_eq!(r.files, 3);
        assert_eq!(r.entries[0].name, "sub");
        assert_eq!(r.entries[0].size, 900);
        assert_eq!(r.ages[0].files, 3); // all just written
        let big = largest_files(root.to_str().unwrap(), Some("Images"), None, 1);
        assert_eq!(big.len(), 1);
        assert_eq!(big[0].size, 600);
        let images = r.types.iter().find(|t| t.category == "Images").unwrap();
        assert_eq!((images.size, images.files), (900, 2));

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

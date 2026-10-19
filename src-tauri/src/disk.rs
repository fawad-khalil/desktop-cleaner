//! Drive listing and directory size scanning.

use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
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
    /// Number of directories that could not be read (permissions etc.).
    pub skipped: u64,
}

#[derive(Default)]
struct Acc {
    size: u64,
    files: u64,
    skipped: u64,
    types: HashMap<&'static str, (u64, u64)>,
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
        self
    }

    fn add_file(&mut self, path: &Path, len: u64) {
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
                acc.add_file(&ent.path(), md.len());
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
                a.add_file(&p, ent.metadata().ok()?.len());
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

    Ok(ScanResult {
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

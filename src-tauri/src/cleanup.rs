//! Dry-run preview and execution of cleanups. Execution only ever moves
//! items to the Recycle Bin / Trash; there is no permanent delete.

use crate::safety::{self, Verdict};
use crate::search::Matcher;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// Everything inside a folder (e.g. a cache), item by item; with
    /// `min_age_days`, only items nothing inside has been used for that long.
    Contents {
        path: String,
        #[serde(default)]
        min_age_days: Option<u64>,
        /// Only items whose path matches.
        #[serde(default)]
        search: Option<crate::search::Filter>,
    },
    /// Files under `path` not used for `min_age_days` and at least `min_size` bytes.
    OldFiles {
        path: String,
        min_age_days: u64,
        min_size: u64,
        /// Only files whose path matches.
        #[serde(default)]
        search: Option<crate::search::Filter>,
    },
}

#[derive(Serialize, Clone)]
pub struct Candidate {
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    /// max(last access, last modification), seconds since the epoch.
    pub last_used: Option<u64>,
    pub source: String,
}

#[derive(Serialize)]
pub struct Refusal {
    pub path: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct Preview {
    pub items: Vec<Candidate>,
    pub total: u64,
    pub protected: Vec<Refusal>,
    pub protected_count: usize,
    /// True when the list was cut to `MAX_ITEMS` (largest kept).
    pub truncated: bool,
}

const MAX_ITEMS: usize = 5000;
const MAX_REFUSALS: usize = 200;

fn epoch(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

pub fn last_used(md: &fs::Metadata) -> Option<u64> {
    let a = md.accessed().ok().and_then(epoch);
    let m = md.modified().ok().and_then(epoch);
    a.max(m)
}

/// `cutoff`: only items last used at or before this time (seconds since the
/// epoch). A folder counts as used when any file inside it was.
fn contents(
    root: &Path,
    cutoff: Option<u64>,
    search: Option<&Matcher>,
    out: &mut Vec<Candidate>,
    refused: &mut Vec<Refusal>,
) {
    if crate::privacy::withheld(root) {
        return;
    }
    let Ok(rd) = fs::read_dir(root) else { return };
    let source = root.to_string_lossy().into_owned();
    let entries: Vec<_> = rd.flatten().collect();
    let results: Vec<Result<Candidate, Refusal>> = entries
        .par_iter()
        .filter_map(|e| {
            let p = e.path();
            if search.is_some_and(|m| !m.is_match(&p.to_string_lossy())) {
                return None;
            }
            let md = fs::symlink_metadata(&p).ok()?;
            if let Verdict::Protected(reason) = safety::check(&p) {
                return Some(Err(Refusal {
                    path: p.to_string_lossy().into_owned(),
                    reason,
                }));
            }
            let (size, used) = if md.is_dir() {
                let (size, newest) = crate::disk::dir_size_and_newest(&p);
                (size, newest.or_else(|| last_used(&md)))
            } else {
                (md.len(), last_used(&md))
            };
            if cutoff.is_some_and(|c| used.is_none_or(|u| u > c)) {
                return None;
            }
            Some(Ok(Candidate {
                path: p.to_string_lossy().into_owned(),
                size,
                is_dir: md.is_dir(),
                last_used: used,
                source: source.clone(),
            }))
        })
        .collect();
    for r in results {
        match r {
            Ok(c) => out.push(c),
            Err(r) => refused.push(r),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn old_files(
    dir: &Path,
    cutoff: u64,
    min_size: u64,
    search: Option<&Matcher>,
    source: &str,
    out: &mut Vec<Candidate>,
    refused: &mut Vec<Refusal>,
) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let p = e.path();
        if let Verdict::Protected(reason) = safety::check(&p) {
            refused.push(Refusal {
                path: p.to_string_lossy().into_owned(),
                reason,
            });
            continue; // never descend into protected trees
        }
        if ft.is_dir() {
            if crate::privacy::withheld(&p) {
                continue;
            }
            old_files(&p, cutoff, min_size, search, source, out, refused);
        } else if ft.is_file() {
            let Ok(md) = e.metadata() else { continue };
            let used = last_used(&md);
            if md.len() >= min_size
                && used.is_some_and(|u| u <= cutoff)
                && search.is_none_or(|m| m.is_match(&p.to_string_lossy()))
            {
                out.push(Candidate {
                    path: p.to_string_lossy().into_owned(),
                    size: md.len(),
                    is_dir: false,
                    last_used: used,
                    source: source.to_string(),
                });
            }
        }
    }
}

/// Lists what the targets would clean. Fails only on an invalid search pattern.
pub fn preview(targets: Vec<Target>) -> Result<Preview, String> {
    let now = epoch(SystemTime::now()).unwrap_or(0);
    let mut items = Vec::new();
    let mut refused = Vec::new();
    for t in targets {
        match t {
            Target::Contents { path, min_age_days, search } => {
                let cutoff = min_age_days.map(|d| now.saturating_sub(d * 86_400));
                let m = crate::search::matcher(search.as_ref())?;
                contents(Path::new(&path), cutoff, m.as_ref(), &mut items, &mut refused)
            }
            Target::OldFiles {
                path,
                min_age_days,
                min_size,
                search,
            } => {
                let m = crate::search::matcher(search.as_ref())?;
                let root = PathBuf::from(&path);
                if let Verdict::Protected(reason) = safety::can_scan(&root) {
                    refused.push(Refusal { path, reason });
                    continue;
                }
                let cutoff = now.saturating_sub(min_age_days * 86_400);
                old_files(&root, cutoff, min_size, m.as_ref(), &path, &mut items, &mut refused);
            }
        }
    }
    // Overlapping targets must not list the same item twice.
    items.sort_by(|a, b| a.path.cmp(&b.path));
    items.dedup_by(|a, b| a.path == b.path);
    items.sort_by(|a, b| b.size.cmp(&a.size));
    let truncated = items.len() > MAX_ITEMS;
    items.truncate(MAX_ITEMS);
    let protected_count = refused.len();
    refused.truncate(MAX_REFUSALS);
    Ok(Preview {
        total: items.iter().map(|c| c.size).sum(),
        items,
        protected: refused,
        protected_count,
        truncated,
    })
}

#[derive(Serialize)]
pub struct CleanResult {
    pub moved: usize,
    pub freed: u64,
    pub failed: Vec<Refusal>,
}

/// Moves the given paths to the Recycle Bin / Trash after re-checking each one
/// against the safety rules.
pub fn execute(paths: Vec<String>) -> CleanResult {
    let mut res = CleanResult {
        moved: 0,
        freed: 0,
        failed: Vec::new(),
    };
    for p in paths {
        let path = PathBuf::from(&p);
        if let Verdict::Protected(reason) = safety::check(&path) {
            res.failed.push(Refusal {
                path: p,
                reason: format!("refused: {reason}"),
            });
            continue;
        }
        let Ok(md) = fs::symlink_metadata(&path) else {
            res.failed.push(Refusal {
                path: p,
                reason: "no longer exists".into(),
            });
            continue;
        };
        let size = if md.is_dir() {
            crate::disk::dir_size(&path).0
        } else {
            md.len()
        };
        match trash::delete(&path) {
            Ok(()) => {
                res.moved += 1;
                res.freed += size;
            }
            Err(e) => res.failed.push(Refusal {
                path: p,
                reason: e.to_string(),
            }),
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_lists_contents_and_old_files_without_deleting() {
        let root = std::env::temp_dir().join(format!("sc-cleanup-test-{}", std::process::id()));
        fs::create_dir_all(root.join("cache/sub")).unwrap();
        fs::write(root.join("cache/a.bin"), vec![0u8; 10]).unwrap();
        fs::write(root.join("cache/sub/b.bin"), vec![0u8; 20]).unwrap();

        let cache = root.join("cache").to_string_lossy().into_owned();
        let contents = |min_age_days, search| Target::Contents { path: cache.clone(), min_age_days, search };
        let old = |min_age_days, min_size, search| Target::OldFiles {
            path: root.to_string_lossy().into_owned(),
            min_age_days,
            min_size,
            search,
        };
        let p = preview(vec![contents(None, None)]).unwrap();
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.total, 30);
        // Just written, so nothing in the cache counts as unused for 7 days.
        assert!(preview(vec![contents(Some(7), None)]).unwrap().items.is_empty());

        // Freshly written files are not "old".
        assert!(preview(vec![old(30, 0, None)]).unwrap().items.is_empty());
        assert_eq!(preview(vec![old(0, 15, None)]).unwrap().items.len(), 1);

        // Search narrows what's listed; an invalid regex is an error.
        let re = |p: &str| Some(crate::search::Filter { pattern: p.into(), regex: true, case_sensitive: false });
        let p = preview(vec![contents(None, re(r"a\.BIN$"))]).unwrap();
        assert_eq!(p.items.len(), 1);
        assert!(p.items[0].path.ends_with("a.bin"));
        assert_eq!(preview(vec![old(0, 0, re(r"sub[/\\]"))]).unwrap().items.len(), 1);
        assert!(preview(vec![old(0, 0, re("(bad"))]).is_err());

        assert!(root.join("cache/a.bin").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn execute_refuses_protected_paths() {
        let home = dirs::home_dir().unwrap().to_string_lossy().into_owned();
        let r = execute(vec![home]);
        assert_eq!(r.moved, 0);
        assert!(r.failed[0].reason.starts_with("refused"));
    }
}

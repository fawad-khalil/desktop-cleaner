//! The disk index: a tree of folder sizes that is kept in memory, saved to
//! the app's data folder, and brought up to date incrementally instead of
//! re-reading the whole drive.
//!
//! - Every folder of at least `KEEP_MIN` and the largest files are nodes;
//!   smaller items are folded into their parent's `own` totals.
//! - On start the saved index is shown at once, then refreshed:
//!   - macOS replays FSEvents history since the saved event id and rereads
//!     only the folders that changed;
//!   - otherwise (or when history is unavailable) folders whose modification
//!     time changed are reread. That misses files growing in place inside
//!     small folders, so a full rescan happens weekly and on request.
//! - While the app is open, a file-system watcher keeps the index live.
//!
//! Everything here only reads the disk (apart from the index file itself).

use crate::disk::{self, AGE_LABELS, Entry, ScanResult, TypeBucket};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const KEEP_MIN: u64 = 1 << 20;
const MAX_FILES_KEPT: usize = 64;
/// How many grandchildren are sent per child for the chart's outer ring.
const MAX_GRANDCHILDREN: usize = 24;
/// Incremental refreshes can drift (see module docs); rescan fully this often.
const FULL_RESCAN_SECS: u64 = 7 * 86_400;
const SCHEMA: u32 = 1;

pub const CATEGORIES: [&str; 11] = [
    "Images", "Videos", "Audio", "Documents", "Archives", "Disk images",
    "Applications & binaries", "Code", "Logs & temp", "Databases", "Other",
];

fn cat_index(c: &str) -> usize {
    CATEGORIES.iter().position(|x| *x == c).unwrap_or(CATEGORIES.len() - 1)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct Stats {
    pub size: u64,
    pub files: u64,
    /// Folders that couldn't be read.
    pub skipped: u64,
    /// Latest last-used time of any file (seconds since the epoch).
    pub newest: u64,
    /// (size, files) per entry of `CATEGORIES`.
    pub types: [(u64, u64); 11],
    /// (size, files) per entry of `AGE_LABELS`.
    pub ages: [(u64, u64); 5],
}

impl Stats {
    fn add(&mut self, o: &Stats) {
        self.size += o.size;
        self.files += o.files;
        self.skipped += o.skipped;
        self.newest = self.newest.max(o.newest);
        for (a, b) in self.types.iter_mut().zip(o.types) {
            a.0 += b.0;
            a.1 += b.1;
        }
        for (a, b) in self.ages.iter_mut().zip(o.ages) {
            a.0 += b.0;
            a.1 += b.1;
        }
    }

    fn file(path: &Path, md: &fs::Metadata) -> Stats {
        disk::bump_progress(false);
        let len = md.len();
        let mut s = Stats { size: len, files: 1, newest: crate::cleanup::last_used(md).unwrap_or(0), ..Default::default() };
        let t = &mut s.types[cat_index(disk::category_for(path))];
        *t = (len, 1);
        s.ages[disk::age_bucket(md)] = (len, 1);
        s
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Node {
    pub name: String,
    pub is_dir: bool,
    /// Modification time in nanoseconds; for folders it changes when
    /// entries are added, removed or renamed.
    mtime: i64,
    /// Folders: their direct small files and small subfolders. Files: the
    /// file itself.
    own: Stats,
    /// Kept subfolders and large files, largest first.
    pub children: Vec<Node>,
    /// `own` plus all children; recomputed, never saved.
    #[serde(skip)]
    pub total: Stats,
}

fn mtime_ns(md: &fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64)
}

fn dir_mtime(p: &Path) -> Option<i64> {
    fs::symlink_metadata(p).ok().map(|m| mtime_ns(&m))
}

impl Node {
    fn dir(name: String, mtime: i64) -> Node {
        Node { name, is_dir: true, mtime, own: Stats::default(), children: Vec::new(), total: Stats::default() }
    }

    fn file(name: String, path: &Path, md: &fs::Metadata) -> Node {
        let own = Stats::file(path, md);
        Node { name, is_dir: false, mtime: mtime_ns(md), total: own.clone(), own, children: Vec::new() }
    }

    /// Recomputes `total` from `own` and the children's totals.
    fn finish(&mut self) {
        let mut t = self.own.clone();
        for c in &self.children {
            t.add(&c.total);
        }
        self.total = t;
        self.children.sort_by(|a, b| b.total.size.cmp(&a.total.size));
    }

    /// Recomputes every total below and including this node (after loading).
    fn recompute(&mut self) {
        self.children.par_iter_mut().for_each(Node::recompute);
        self.finish();
    }

    /// Fills this folder from freshly read files and subfolders, keeping the
    /// big ones as nodes and folding the rest into `own`.
    fn assemble(&mut self, mut files: Vec<Node>, subs: Vec<Node>, skipped: u64) {
        self.own = Stats { skipped, ..Default::default() };
        self.children.clear();
        files.sort_by(|a, b| b.own.size.cmp(&a.own.size));
        for (i, f) in files.into_iter().enumerate() {
            if f.own.size >= KEEP_MIN && i < MAX_FILES_KEPT {
                self.children.push(f);
            } else {
                self.own.add(&f.own);
            }
        }
        for s in subs {
            if s.total.size >= KEEP_MIN {
                self.children.push(s);
            } else {
                self.own.add(&s.total);
            }
        }
        self.finish();
    }

    fn find(&self, rel: &Path) -> Option<&Node> {
        let mut n = self;
        for c in rel.components() {
            let name = c.as_os_str().to_string_lossy();
            n = n.children.iter().find(|x| x.is_dir && x.name == *name)?;
        }
        Some(n)
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Reads a folder's direct entries: (files as nodes, subfolder paths).
/// File details are read in parallel; cache folders can hold 100k files.
fn read_entries(dir: &Path) -> Option<(Vec<Node>, Vec<PathBuf>)> {
    let ents: Vec<fs::DirEntry> = fs::read_dir(dir).ok()?.flatten().collect();
    let items: Vec<(Option<Node>, Option<PathBuf>)> = ents
        .par_iter()
        .filter_map(|ent| {
            let ft = ent.file_type().ok()?;
            // Symlinks and junctions are never followed.
            if ft.is_symlink() {
                return None;
            }
            let p = ent.path();
            if ft.is_dir() {
                (!disk::skip_dir(&p) && !disk::other_device(&p)).then_some((None, Some(p)))
            } else if ft.is_file() {
                let md = ent.metadata().ok()?;
                Some((Some(Node::file(ent.file_name().to_string_lossy().into_owned(), &p, &md)), None))
            } else {
                None
            }
        })
        .collect();
    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    for (f, d) in items {
        files.extend(f);
        subdirs.extend(d);
    }
    Some((files, subdirs))
}

/// Builds the tree for `dir` from scratch.
fn walk(dir: &Path, name: String) -> Node {
    let mut node = Node::dir(name, dir_mtime(dir).unwrap_or(0));
    let Some((files, subdirs)) = read_entries(dir) else {
        node.assemble(Vec::new(), Vec::new(), 1);
        return node;
    };
    let subs = subdirs.par_iter().map(|d| walk(d, file_name(d))).collect();
    node.assemble(files, subs, 0);
    node
}

/// How a refresh decides which folders to reread.
enum Mode<'a> {
    /// Reread folders whose modification time changed.
    Mtime,
    /// Reread exactly these folders; `ancestors` are the folders above them.
    Dirty { dirty: &'a HashSet<PathBuf>, ancestors: &'a HashSet<PathBuf> },
}

/// Brings `node` (the folder at `path`) up to date, reusing everything that
/// hasn't changed.
fn refresh(node: Node, path: &Path, mode: &Mode) -> Node {
    let changed = match mode {
        Mode::Mtime => dir_mtime(path) != Some(node.mtime),
        Mode::Dirty { dirty, .. } => dirty.contains(path),
    };
    if changed {
        return reread(node, path, mode);
    }
    if let Mode::Dirty { ancestors, .. } = mode {
        if !ancestors.contains(path) {
            return node;
        }
    }
    // Unchanged folder: refresh what's below it.
    let Node { name, is_dir, mtime, own, children, .. } = node;
    let mtime_mode = matches!(mode, Mode::Mtime);
    let kids: Vec<Node> = children
        .into_par_iter()
        .filter_map(|c| {
            let cp = path.join(&c.name);
            if c.is_dir {
                Some(refresh(c, &cp, mode))
            } else if mtime_mode {
                // Big files are cheap to re-check and catch in-place growth.
                let md = fs::symlink_metadata(&cp).ok()?;
                if md.len() == c.own.size && mtime_ns(&md) == c.mtime { Some(c) } else { Some(Node::file(c.name, &cp, &md)) }
            } else {
                Some(c)
            }
        })
        .collect();
    let mut n = Node { name, is_dir, mtime, own, children: Vec::new(), total: Stats::default() };
    for k in kids {
        if k.total.size >= KEEP_MIN {
            n.children.push(k);
        } else {
            n.own.add(&k.total);
        }
    }
    n.finish();
    n
}

/// Rereads a folder's entries. Subfolders that were already known are
/// refreshed (not rescanned); new or previously small ones are walked.
fn reread(node: Node, path: &Path, mode: &Mode) -> Node {
    let mut n = Node::dir(node.name, dir_mtime(path).unwrap_or(0));
    let Some((files, subdirs)) = read_entries(path) else {
        n.assemble(Vec::new(), Vec::new(), 1);
        return n;
    };
    let mut known: HashMap<String, Node> =
        node.children.into_iter().filter(|c| c.is_dir).map(|c| (c.name.clone(), c)).collect();
    let jobs: Vec<(PathBuf, Option<Node>)> = subdirs
        .into_iter()
        .map(|d| {
            let old = known.remove(&file_name(&d));
            (d, old)
        })
        .collect();
    let subs = jobs
        .into_par_iter()
        .map(|(d, old)| match old {
            Some(o) => refresh(o, &d, mode),
            None => walk(&d, file_name(&d)),
        })
        .collect();
    n.assemble(files, subs, 0);
    n
}

/// Maps changed paths to the nearest folder in the tree, plus the set of
/// folders above them, for `Mode::Dirty`.
fn resolve_dirty(root: &Path, tree: &Node, changed: &[PathBuf]) -> (HashSet<PathBuf>, HashSet<PathBuf>) {
    let mut dirty = HashSet::new();
    let mut ancestors = HashSet::new();
    for p in changed {
        let Ok(rel) = p.strip_prefix(root) else { continue };
        let mut node = tree;
        let mut at = root.to_path_buf();
        for c in rel.components() {
            let Component::Normal(name) = c else { continue };
            let name = name.to_string_lossy();
            match node.children.iter().find(|x| x.is_dir && x.name == *name) {
                Some(next) => {
                    ancestors.insert(at.clone());
                    at.push(&*name);
                    node = next;
                }
                None => break,
            }
        }
        dirty.insert(at);
    }
    (dirty, ancestors)
}

// ---------------------------------------------------------------------------
// Trees in memory and on disk

struct Tree {
    root: PathBuf,
    node: Node,
    full_scan_at: u64,
    updated_at: u64,
    /// FSEvents id the index is current up to (macOS).
    event_id: Option<u64>,
}

static TREES: Mutex<Vec<Tree>> = Mutex::new(Vec::new());
const MAX_TREES: usize = 8;
/// One refresh or full scan at a time.
static WORK: Mutex<()> = Mutex::new(());
/// Where index files are saved; set at startup (tests use a temp folder).
static STORE: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_store_dir(dir: PathBuf) {
    *STORE.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir);
}

#[derive(Serialize, Deserialize)]
struct Saved {
    schema: u32,
    root: String,
    full_scan_at: u64,
    updated_at: u64,
    event_id: Option<u64>,
    tree: Node,
}

fn store_file(root: &Path) -> Option<PathBuf> {
    let dir = STORE.lock().ok()?.clone()?;
    let s = root.to_string_lossy();
    // Readable prefix plus a hash so different roots never collide.
    let readable: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(40).collect();
    let hash = s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    Some(dir.join(format!("{readable}-{hash:016x}.idx")))
}

fn save(t: &Tree) -> anyhow::Result<()> {
    let Some(file) = store_file(&t.root) else { return Ok(()) };
    fs::create_dir_all(file.parent().unwrap())?;
    let saved = Saved {
        schema: SCHEMA,
        root: t.root.to_string_lossy().into_owned(),
        full_scan_at: t.full_scan_at,
        updated_at: t.updated_at,
        event_id: t.event_id,
        tree: t.node.clone(),
    };
    let bytes = rmp_serde::to_vec(&saved)?;
    let tmp = file.with_extension("tmp");
    let mut gz = flate2::write::GzEncoder::new(fs::File::create(&tmp)?, flate2::Compression::fast());
    gz.write_all(&bytes)?;
    gz.finish()?;
    fs::rename(tmp, file)?;
    Ok(())
}

fn load(root: &Path) -> Option<Tree> {
    let file = store_file(root)?;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(fs::File::open(file).ok()?).read_to_end(&mut bytes).ok()?;
    let s: Saved = rmp_serde::from_slice(&bytes).ok()?;
    if s.schema != SCHEMA || Path::new(&s.root) != root {
        return None;
    }
    let mut node = s.tree;
    node.recompute();
    Some(Tree { root: root.to_path_buf(), node, full_scan_at: s.full_scan_at, updated_at: s.updated_at, event_id: s.event_id })
}

fn put(t: Tree) {
    let mut g = TREES.lock().unwrap_or_else(|e| e.into_inner());
    // A tree replaces any it contains.
    g.retain(|x| !x.root.starts_with(&t.root));
    g.push(t);
    if g.len() > MAX_TREES {
        g.remove(0);
    }
}

fn result(t: &Tree, path: &Path) -> Option<ScanResult> {
    let node = t.node.find(path.strip_prefix(&t.root).ok()?)?;
    let entry = |n: &Node, parent: &Path, kids: Vec<Entry>| Entry {
        name: n.name.clone(),
        path: parent.join(&n.name).to_string_lossy().into_owned(),
        size: n.total.size,
        is_dir: n.is_dir,
        files: n.total.files,
        children: kids,
    };
    let entries = node
        .children
        .iter()
        .map(|c| {
            let cp = path.join(&c.name);
            let kids = c.children.iter().take(MAX_GRANDCHILDREN).map(|g| entry(g, &cp, Vec::new())).collect();
            entry(c, path, kids)
        })
        .collect();
    let mut types: Vec<TypeBucket> = CATEGORIES
        .iter()
        .zip(node.total.types)
        .filter(|(_, (s, f))| *s > 0 || *f > 0)
        .map(|(c, (size, files))| TypeBucket { category: c.to_string(), size, files })
        .collect();
    types.sort_by(|a, b| b.size.cmp(&a.size));
    let ages = AGE_LABELS
        .iter()
        .zip(node.total.ages)
        .map(|(c, (size, files))| TypeBucket { category: c.to_string(), size, files })
        .collect();
    Some(ScanResult {
        path: path.to_string_lossy().into_owned(),
        total: node.total.size,
        files: node.total.files,
        entries,
        types,
        ages,
        skipped: node.total.skipped,
        root: t.root.to_string_lossy().into_owned(),
        updated_at: t.updated_at,
        full_scan_at: t.full_scan_at,
    })
}

/// The tree containing `path` from memory, else from the saved index of
/// `path` or any folder above it.
fn lookup(path: &Path) -> Option<ScanResult> {
    {
        let g = TREES.lock().ok()?;
        if let Some(r) = g.iter().rev().find_map(|t| result(t, path)) {
            return Some(r);
        }
    }
    let t = path.ancestors().find_map(load)?;
    let r = result(&t, path);
    put(t);
    r
}

fn current_event_id() -> Option<u64> {
    #[cfg(target_os = "macos")]
    return Some(fsevents::current_id());
    #[cfg(not(target_os = "macos"))]
    None
}

fn full_scan(root: &Path) -> anyhow::Result<Tree> {
    let md = fs::symlink_metadata(root)?;
    if !md.is_dir() {
        anyhow::bail!("{} is not a directory", root.display());
    }
    disk::bump_progress(true);
    disk::set_scan_root(&md);
    // Taken before walking so nothing that changes during the walk is missed.
    let event_id = current_event_id();
    let node = walk(root, file_name(root));
    let t = now();
    Ok(Tree { root: root.to_path_buf(), node, full_scan_at: t, updated_at: t, event_id })
}

/// Shows `path`: from memory or the saved index when possible (unless
/// `fresh`), otherwise by scanning it. A fresh scan of a folder inside a
/// known tree is merged into that tree.
pub fn scan(path: &str, fresh: bool) -> anyhow::Result<ScanResult> {
    let path = PathBuf::from(path);
    if !fresh {
        if let Some(r) = lookup(&path) {
            return Ok(r);
        }
    }
    let _w = WORK.lock().unwrap_or_else(|e| e.into_inner());
    let scanned = full_scan(&path)?;
    let mut g = TREES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = g.iter_mut().find(|t| path != t.root && path.starts_with(&t.root)) {
        let rel = path.strip_prefix(&t.root).unwrap().to_path_buf();
        if graft(&mut t.node, &rel, scanned.node.clone()) {
            t.updated_at = now();
            let r = result(t, &path);
            let _ = save(t);
            return r.ok_or_else(|| anyhow::anyhow!("lost {}", path.display()));
        }
    }
    drop(g);
    let _ = save(&scanned);
    let r = result(&scanned, &path);
    put(scanned);
    r.ok_or_else(|| anyhow::anyhow!("lost {}", path.display()))
}

/// Replaces the folder at `rel` with `new`, fixing the totals above it.
fn graft(node: &mut Node, rel: &Path, new: Node) -> bool {
    let mut comps = rel.components().filter_map(|c| match c {
        Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
        _ => None,
    });
    let Some(first) = comps.next() else { return false };
    let rest: PathBuf = comps.collect();
    let Some(i) = node.children.iter().position(|c| c.is_dir && c.name == first) else { return false };
    let ok = if rest.as_os_str().is_empty() {
        node.children[i] = new;
        true
    } else {
        graft(&mut node.children[i], &rest, new)
    };
    if ok {
        node.finish();
    }
    ok
}

/// How the last refresh worked, for the UI.
#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Replayed file-system change history (macOS FSEvents).
    Events,
    /// Compared folder modification times.
    Mtime,
    /// Rescanned everything (first scan, weekly, or history unavailable and
    /// no saved index).
    Full,
}

#[derive(Serialize)]
pub struct Refreshed {
    pub result: ScanResult,
    pub method: Method,
}

/// Brings the index containing `path` up to date and returns `path`.
pub fn refresh_index(path: &str) -> anyhow::Result<Refreshed> {
    let path = PathBuf::from(path);
    let _w = WORK.lock().unwrap_or_else(|e| e.into_inner());
    // Take the tree out so lookups stay answerable from a copy meanwhile.
    let (root, node, full_scan_at, event_id) = {
        let g = TREES.lock().unwrap_or_else(|e| e.into_inner());
        match g.iter().rev().find(|t| path.starts_with(&t.root)) {
            Some(t) => (t.root.clone(), t.node.clone(), t.full_scan_at, t.event_id),
            None => {
                drop(g);
                let Some(t) = path.ancestors().find_map(load) else {
                    drop(_w);
                    let result = scan(&path.to_string_lossy(), true)?;
                    return Ok(Refreshed { result, method: Method::Full });
                };
                let v = (t.root.clone(), t.node.clone(), t.full_scan_at, t.event_id);
                put(t);
                v
            }
        }
    };
    if now().saturating_sub(full_scan_at) > FULL_RESCAN_SECS {
        drop(_w);
        let result = scan(&root.to_string_lossy(), true)?;
        let result = if path == root { result } else { lookup(&path).unwrap_or(result) };
        return Ok(Refreshed { result, method: Method::Full });
    }
    if let Ok(md) = fs::symlink_metadata(&root) {
        disk::set_scan_root(&md);
    }
    disk::bump_progress(true);
    let new_event_id = current_event_id();
    let changes = event_id.and_then(|id| changes_since(&root, id));
    let (node, method) = match changes {
        Some(paths) => {
            let (dirty, ancestors) = resolve_dirty(&root, &node, &paths);
            (refresh(node, &root, &Mode::Dirty { dirty: &dirty, ancestors: &ancestors }), Method::Events)
        }
        None => (refresh(node, &root, &Mode::Mtime), Method::Mtime),
    };
    let t = Tree { root: root.clone(), node, full_scan_at, updated_at: now(), event_id: new_event_id };
    let _ = save(&t);
    put(t);
    let result = lookup(&path).ok_or_else(|| anyhow::anyhow!("lost {}", path.display()))?;
    Ok(Refreshed { result, method })
}

/// Applies changes reported by the live watcher to the index for `root`.
/// Returns false when there's no such index.
pub fn apply_changes(root: &Path, changed: &[PathBuf]) -> bool {
    let _w = WORK.lock().unwrap_or_else(|e| e.into_inner());
    let (node, mut meta) = {
        let g = TREES.lock().unwrap_or_else(|e| e.into_inner());
        let Some(t) = g.iter().find(|t| t.root == root) else { return false };
        (t.node.clone(), (t.full_scan_at, t.event_id))
    };
    if let Ok(md) = fs::symlink_metadata(root) {
        disk::set_scan_root(&md);
    }
    meta.1 = current_event_id().or(meta.1);
    let (dirty, ancestors) = resolve_dirty(root, &node, changed);
    let node = refresh(node, root, &Mode::Dirty { dirty: &dirty, ancestors: &ancestors });
    let t = Tree { root: root.to_path_buf(), node, full_scan_at: meta.0, updated_at: now(), event_id: meta.1 };
    let mut g = TREES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = g.iter_mut().find(|x| x.root == root) {
        *slot = t;
    }
    true
}

/// Saves the index for `root` (the watcher calls this now and then).
pub fn persist(root: &Path) {
    let g = TREES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = g.iter().find(|t| t.root == root) {
        let _ = save(t);
    }
}

/// Roots of the trees currently in memory.
pub fn roots() -> Vec<PathBuf> {
    TREES.lock().map(|g| g.iter().map(|t| t.root.clone()).collect()).unwrap_or_default()
}

/// Paths changed under `root` since FSEvents id `since`, or `None` when
/// the history can't be trusted and folder times must be compared instead.
fn changes_since(root: &Path, since: u64) -> Option<Vec<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        let raw = fsevents::changes_since(root, since, std::time::Duration::from_secs(20)).ok()?;
        // Changes on the data volume can be reported under its real mount
        // point; map them back to the firmlinked paths the tree uses.
        Some(
            raw.into_iter()
                .map(|p| match p.strip_prefix("/System/Volumes/Data") {
                    Ok(rest) if root == Path::new("/") => Path::new("/").join(rest),
                    _ => p,
                })
                .collect(),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, since);
        None
    }
}

#[cfg(target_os = "macos")]
pub mod fsevents {
    //! Replays FSEvents history: every folder changed since an event id.

    use fsevent_sys as fse;
    use fsevent_sys::core_foundation as cf;
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRunLoopRunInMode(mode: cf::CFStringRef, seconds: f64, return_after_source_handled: u8) -> i32;
    }

    pub fn current_id() -> u64 {
        unsafe { fse::FSEventsGetCurrentEventId() }
    }

    #[derive(Default)]
    struct Ctx {
        paths: Vec<PathBuf>,
        done: bool,
        /// Events were dropped or coalesced; history isn't usable.
        unreliable: bool,
    }

    extern "C" fn callback(
        _stream: fse::FSEventStreamRef,
        info: *mut c_void,
        n: usize,
        paths: *mut c_void,
        flags: *const fse::FSEventStreamEventFlags,
        _ids: *const fse::FSEventStreamEventId,
    ) {
        let ctx = unsafe { &mut *(info as *mut Ctx) };
        let paths = paths as *const *const c_char;
        for i in 0..n {
            let f = unsafe { *flags.add(i) };
            if f & fse::kFSEventStreamEventFlagHistoryDone != 0 {
                ctx.done = true;
                continue;
            }
            let bad = fse::kFSEventStreamEventFlagMustScanSubDirs
                | fse::kFSEventStreamEventFlagUserDropped
                | fse::kFSEventStreamEventFlagKernelDropped
                | fse::kFSEventStreamEventFlagEventIdsWrapped
                | fse::kFSEventStreamEventFlagRootChanged;
            if f & bad != 0 {
                ctx.unreliable = true;
            }
            let p = unsafe { CStr::from_ptr(*paths.add(i)) };
            ctx.paths.push(PathBuf::from(p.to_string_lossy().trim_end_matches('/')));
        }
    }

    pub fn changes_since(root: &Path, since: u64, timeout: Duration) -> Result<Vec<PathBuf>, String> {
        let c_root = CString::new(root.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let mut ctx = Box::new(Ctx::default());
        unsafe {
            let cf_path = cf::CFStringCreateWithCString(cf::kCFAllocatorDefault, c_root.as_ptr(), cf::kCFStringEncodingUTF8);
            let paths = cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 1, &cf::kCFTypeArrayCallBacks);
            cf::CFArrayAppendValue(paths, cf_path);
            let context = fse::FSEventStreamContext {
                version: 0,
                info: &mut *ctx as *mut Ctx as *mut c_void,
                retain: None,
                release: None,
                copy_description: None,
            };
            let stream = fse::FSEventStreamCreate(
                cf::kCFAllocatorDefault,
                callback,
                &context,
                paths,
                since,
                0.0,
                fse::kFSEventStreamCreateFlagNoDefer,
            );
            fse::FSEventStreamScheduleWithRunLoop(stream, cf::CFRunLoopGetCurrent(), cf::kCFRunLoopDefaultMode);
            let started = fse::FSEventStreamStart(stream) != 0;
            let t0 = Instant::now();
            while started && !ctx.done && t0.elapsed() < timeout {
                CFRunLoopRunInMode(cf::kCFRunLoopDefaultMode, 0.2, 1);
            }
            fse::FSEventStreamStop(stream);
            fse::FSEventStreamInvalidate(stream);
            fse::FSEventStreamRelease(stream);
            cf::CFRelease(paths);
            cf::CFRelease(cf_path);
            if !started {
                return Err("couldn't start an FSEvents stream".into());
            }
        }
        if !ctx.done {
            return Err("FSEvents history didn't finish in time".into());
        }
        if ctx.unreliable {
            return Err("FSEvents history is incomplete".into());
        }
        let mut paths = std::mem::take(&mut ctx.paths);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: usize = 1 << 20;
    const MB: u64 = M as u64;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("dc-index-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        // Canonical, so it matches the paths FSEvents reports (/private/var…).
        p.canonicalize().unwrap()
    }

    fn sample(root: &Path) {
        fs::create_dir_all(root.join("sub/deeper")).unwrap();
        fs::create_dir_all(root.join("small")).unwrap();
        fs::write(root.join("a.txt"), vec![0u8; M]).unwrap();
        fs::write(root.join("sub/b.png"), vec![0u8; 3 * M]).unwrap();
        fs::write(root.join("sub/deeper/c.png"), vec![0u8; 6 * M]).unwrap();
        fs::write(root.join("sub/deeper/tiny.txt"), vec![0u8; 10]).unwrap();
        fs::write(root.join("small/x.txt"), vec![0u8; 100]).unwrap();
    }

    fn total_of(root: &Path, sub: &str) -> u64 {
        let g = TREES.lock().unwrap();
        let t = g.iter().find(|t| t.root == root).unwrap();
        t.node.find(Path::new(sub)).unwrap().total.size
    }

    #[test]
    fn scan_sizes_children_types_and_drilldown() {
        let root = temp("scan");
        sample(&root);
        let r = scan(root.to_str().unwrap(), true).unwrap();
        assert_eq!(r.total, 10 * MB + 110);
        assert_eq!(r.files, 5);
        assert_eq!(r.entries[0].name, "sub");
        assert_eq!(r.entries[0].size, 9 * MB + 10);
        let kids: Vec<_> = r.entries[0].children.iter().map(|e| (e.name.as_str(), e.size)).collect();
        assert_eq!(kids, [("deeper", 6 * MB + 10), ("b.png", 3 * MB)]);
        assert_eq!(r.ages[0].files, 5);
        let images = r.types.iter().find(|t| t.category == "Images").unwrap();
        assert_eq!((images.size, images.files), (9 * MB, 2));

        // Drilling down comes from memory, small files still counted.
        let d = scan(root.join("sub/deeper").to_str().unwrap(), false).unwrap();
        assert_eq!(d.total, 6 * MB + 10);
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.types.iter().map(|t| t.files).sum::<u64>(), 2);

        // A fresh scan of a subfolder is grafted in, fixing totals above.
        fs::write(root.join("sub/deeper/d.png"), vec![0u8; 2 * M]).unwrap();
        scan(root.join("sub/deeper").to_str().unwrap(), true).unwrap();
        assert_eq!(total_of(&root, ""), 12 * MB + 110);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn index_saves_and_loads() {
        let root = temp("save");
        let store = temp("store");
        sample(&root);
        set_store_dir(store.clone());
        let before = scan(root.to_str().unwrap(), true).unwrap();
        // Forget the in-memory copy; the next lookup must come from disk.
        TREES.lock().unwrap().retain(|t| t.root != root);
        let after = scan(root.join("sub").to_str().unwrap(), false).unwrap();
        assert_eq!(after.total, 9 * MB + 10);
        assert_eq!(after.updated_at, before.updated_at);
        assert_eq!(after.root, root.to_string_lossy());
        // A file from another schema or root is ignored.
        let f = store_file(&root).unwrap();
        assert!(load(&root).is_some());
        assert!(load(&root.join("sub")).is_none());
        fs::write(&f, b"garbage").unwrap();
        assert!(load(&root).is_none());
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&store).unwrap();
    }

    #[test]
    fn mtime_refresh_picks_up_added_and_removed_items() {
        let root = temp("mtime");
        sample(&root);
        let r = root.to_string_lossy().into_owned();
        scan(&r, true).unwrap();
        // New folder with a big file, a removed folder, and a big file growing in place.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::create_dir_all(root.join("new")).unwrap();
        fs::write(root.join("new/big.bin"), vec![0u8; 4 * M]).unwrap();
        fs::remove_dir_all(root.join("small")).unwrap();
        fs::write(root.join("sub/b.png"), vec![0u8; 5 * M]).unwrap();
        let tree = TREES.lock().unwrap().iter().find(|t| t.root == root).unwrap().node.clone();
        let node = refresh(tree, &root, &Mode::Mtime);
        assert_eq!(node.total.size, 16 * MB + 10);
        assert!(node.find(Path::new("new")).is_some());
        assert_eq!(node.find(Path::new("sub")).unwrap().total.size, 11 * MB + 10);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dirty_refresh_rereads_only_changed_folders() {
        let root = temp("dirty");
        sample(&root);
        scan(root.to_str().unwrap(), true).unwrap();
        let tree = TREES.lock().unwrap().iter().find(|t| t.root == root).unwrap().node.clone();
        // A small folder grows a big file in place (no parent mtime change),
        // and an unrelated folder changes without being reported.
        fs::write(root.join("small/x.txt"), vec![0u8; 3 * M]).unwrap();
        fs::write(root.join("sub/deeper/ignored.bin"), vec![0u8; 2 * M]).unwrap();
        let changed = vec![root.join("small")];
        let (dirty, ancestors) = resolve_dirty(&root, &tree, &changed);
        // "small" isn't a node (under 1 MB), so its parent is reread.
        assert!(dirty.contains(&root));
        let node = refresh(tree, &root, &Mode::Dirty { dirty: &dirty, ancestors: &ancestors });
        assert_eq!(node.find(Path::new("small")).unwrap().total.size, 3 * MB);
        // Not reported, so not reread.
        assert_eq!(node.find(Path::new("sub/deeper")).unwrap().total.size, 6 * MB + 10);
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn fsevents_history_reports_changed_folders() {
        let root = temp("fsevents");
        let since = fsevents::current_id();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/f.txt"), b"hi").unwrap();
        // FSEvents writes its history asynchronously.
        std::thread::sleep(std::time::Duration::from_secs(2));
        let paths = fsevents::changes_since(&root, since, std::time::Duration::from_secs(10)).unwrap();
        assert!(paths.iter().any(|p| p.starts_with(&root)), "{paths:?}");
        fs::remove_dir_all(&root).unwrap();
    }
}

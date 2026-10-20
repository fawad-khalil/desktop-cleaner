//! Keeps the disk index live while the app is open: watches each indexed
//! root for changes and applies them in debounced batches.
//!
//! Not used on Linux, where watching a whole drive with inotify would need
//! a watch per folder; there the index is refreshed on start and on request.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Quiet time before a batch of changes is applied, the longest a change
/// waits while events keep coming, and the minimum gap between batches for
/// one root. Busy folders (browser caches) change constantly and rereading
/// them isn't free, so batches are kept infrequent.
const SETTLE: Duration = Duration::from_secs(5);
const MAX_WAIT: Duration = Duration::from_secs(60);
const MIN_GAP: Duration = Duration::from_secs(30);
const SAVE_EVERY: Duration = Duration::from_secs(60);

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn start(on_change: impl Fn(&str) + Send + 'static, ignore: Option<PathBuf>) {
    use notify::{EventKind, RecursiveMode, Watcher};
    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel::<notify::Event>();
        let Ok(mut watcher) = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(e) = res {
                let _ = tx.send(e);
            }
        }) else {
            return;
        };
        let mut watched: HashSet<PathBuf> = HashSet::new();
        let mut pending: HashMap<PathBuf, (HashSet<PathBuf>, Instant, Instant)> = HashMap::new();
        let mut unsaved: HashMap<PathBuf, Instant> = HashMap::new();
        let mut last_applied: HashMap<PathBuf, Instant> = HashMap::new();
        loop {
            // Watch roots as they get indexed.
            for root in crate::index::roots() {
                if watched.insert(root.clone()) && watcher.watch(&root, RecursiveMode::Recursive).is_err() {
                    watched.remove(&root);
                }
            }
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(e) if !matches!(e.kind, EventKind::Access(_)) => {
                    let now = Instant::now();
                    for p in e.paths {
                        if ignore.as_ref().is_some_and(|i| p.starts_with(i)) {
                            continue;
                        }
                        // Deepest watched root containing the path.
                        let Some(root) = watched.iter().filter(|r| p.starts_with(r)).max_by_key(|r| r.as_os_str().len()) else { continue };
                        let entry = pending.entry(root.clone()).or_insert_with(|| (HashSet::new(), now, now));
                        // The parent folder is what needs rereading.
                        entry.0.insert(p.parent().map(|x| x.to_path_buf()).unwrap_or(p));
                        entry.2 = now;
                    }
                }
                _ => {}
            }
            let now = Instant::now();
            let ready: Vec<PathBuf> = pending
                .iter()
                .filter(|(r, (_, first, last))| {
                    (now - *last >= SETTLE || now - *first >= MAX_WAIT)
                        && last_applied.get(*r).is_none_or(|t| now - *t >= MIN_GAP)
                })
                .map(|(r, _)| r.clone())
                .collect();
            for root in ready {
                let (paths, _, _) = pending.remove(&root).unwrap();
                let paths: Vec<PathBuf> = paths.into_iter().collect();
                last_applied.insert(root.clone(), now);
                if crate::index::apply_changes(&root, &paths) {
                    unsaved.entry(root.clone()).or_insert(now);
                    on_change(&root.to_string_lossy());
                }
            }
            let due: Vec<PathBuf> = unsaved.iter().filter(|(_, t)| now - **t >= SAVE_EVERY).map(|(r, _)| r.clone()).collect();
            for root in due {
                unsaved.remove(&root);
                crate::index::persist(&root);
            }
        }
    });
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn start(_on_change: impl Fn(&str) + Send + 'static, _ignore: Option<PathBuf>) {}

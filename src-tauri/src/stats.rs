//! Running totals of space freed: this session (since the app started) and
//! all time, with a short history. Only items actually moved to the Trash /
//! Recycle Bin count, never previews.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

const SCHEMA: u32 = 1;
const HISTORY: usize = 50;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    /// Seconds since the epoch.
    pub at: u64,
    pub bytes: u64,
    pub items: u64,
    /// "cleanup" or "app" (an app removed with its leftovers).
    pub kind: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Store {
    schema: u32,
    total_bytes: u64,
    total_items: u64,
    first_at: Option<u64>,
    /// Newest first.
    history: Vec<Event>,
}

#[derive(Serialize, Debug)]
pub struct Summary {
    pub session_bytes: u64,
    pub session_items: u64,
    pub total_bytes: u64,
    pub total_items: u64,
    /// When the first cleanup was recorded.
    pub since: Option<u64>,
    pub history: Vec<Event>,
}

static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
static SESSION: Mutex<(u64, u64)> = Mutex::new((0, 0));

pub fn set_file(p: PathBuf) {
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
}

fn load(file: &Option<PathBuf>) -> Store {
    file.as_ref()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice::<Store>(&b).ok())
        .filter(|s| s.schema == SCHEMA)
        .unwrap_or(Store { schema: SCHEMA, ..Default::default() })
}

/// Adds a successful cleanup to the session and all-time totals.
pub fn record(kind: &str, bytes: u64, items: u64) {
    if items == 0 {
        return;
    }
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    {
        let mut s = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        s.0 += bytes;
        s.1 += items;
    }
    let file = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = load(&file);
    store.total_bytes += bytes;
    store.total_items += items;
    store.first_at.get_or_insert(at);
    store.history.insert(0, Event { at, bytes, items, kind: kind.to_string() });
    store.history.truncate(HISTORY);
    if let Some(f) = file.as_ref() {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = f.with_extension("tmp");
        if serde_json::to_vec_pretty(&store).ok().and_then(|b| std::fs::write(&tmp, b).ok()).is_some() {
            let _ = std::fs::rename(tmp, f);
        }
    }
}

pub fn summary() -> Summary {
    let (session_bytes, session_items) = *SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let store = load(&FILE.lock().unwrap_or_else(|e| e.into_inner()));
    Summary {
        session_bytes,
        session_items,
        total_bytes: store.total_bytes,
        total_items: store.total_items,
        since: store.first_at,
        history: store.history,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_session_and_all_time_totals() {
        let dir = std::env::temp_dir().join(format!("dc-stats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("stats.json");
        set_file(file.clone());
        // An earlier session's total already on disk.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, r#"{"schema":1,"total_bytes":1000,"total_items":2,"first_at":5,"history":[]}"#).unwrap();

        record("cleanup", 0, 0); // nothing moved: ignored
        record("cleanup", 300, 3);
        record("app", 700, 1);
        let s = summary();
        assert_eq!((s.session_bytes, s.session_items), (1000, 4));
        assert_eq!((s.total_bytes, s.total_items), (2000, 6));
        assert_eq!(s.since, Some(5));
        assert_eq!(s.history.len(), 2);
        assert_eq!(s.history[0].kind, "app"); // newest first
        assert!(file.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

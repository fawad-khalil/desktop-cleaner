mod apps;
mod caches;
mod cleanup;
mod disk;
mod format;
mod index;
mod leftovers;
mod processes;
mod safety;
mod stats;
mod uninstall;
mod watch;

use processes::ProcState;
use tauri::{Emitter, Manager, State};

/// Runs blocking work off the main thread and flattens errors to strings
/// for the frontend.
async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_drives() -> Result<Vec<disk::DriveInfo>, String> {
    blocking(|| Ok(disk::list_drives())).await
}

#[tauri::command]
async fn scan_dir(path: String, fresh: Option<bool>) -> Result<disk::ScanResult, String> {
    blocking(move || index::scan(&path, fresh.unwrap_or(false))).await
}

/// Brings the saved index containing `path` up to date (FSEvents history on
/// macOS, folder times elsewhere, a full rescan weekly) and returns `path`.
#[tauri::command]
async fn refresh_index(path: String) -> Result<index::Refreshed, String> {
    blocking(move || index::refresh_index(&path)).await
}

#[tauri::command]
fn scan_progress() -> u64 {
    disk::scan_progress()
}

/// Opens System Settings at Privacy & Security › Full Disk Access (macOS),
/// where the user can let the app read protected folders.
#[tauri::command]
fn open_full_disk_access() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn largest_files(
    path: String,
    category: Option<String>,
    age: Option<usize>,
    limit: Option<usize>,
) -> Result<Vec<disk::FileItem>, String> {
    blocking(move || {
        Ok(disk::largest_files(&path, category.as_deref(), age, limit.unwrap_or(100)))
    })
    .await
}

#[tauri::command]
fn list_processes(state: State<'_, ProcState>) -> processes::ProcSnapshot {
    processes::snapshot(&state)
}

/// Asks the given processes to quit. System processes are refused.
#[tauri::command]
fn quit_processes(state: State<'_, ProcState>, pids: Vec<u32>) -> Result<u32, String> {
    processes::quit(&state, &pids).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_caches() -> Result<Vec<caches::CacheInfo>, String> {
    blocking(|| Ok(caches::list())).await
}

#[tauri::command]
async fn list_apps() -> Result<Vec<apps::AppInfo>, String> {
    blocking(|| Ok(apps::list())).await
}

/// Support files an app leaves in the user's Library (macOS).
#[tauri::command]
async fn app_leftovers(location: String) -> Result<Vec<uninstall::Leftover>, String> {
    blocking(move || Ok(uninstall::leftovers(&location))).await
}

/// Removes an app: to the Trash on macOS, via its own uninstaller on
/// Windows, or returns the package-manager command on Linux. `target` is
/// the app's location (macOS, Linux) or display name (Windows).
#[tauri::command]
async fn remove_app(target: String, leftovers: Vec<String>) -> Result<uninstall::Outcome, String> {
    blocking(move || {
        let r = uninstall::remove(&target, &leftovers)?;
        stats::record("app", r.freed, r.moved.len() as u64);
        Ok(r)
    })
    .await
}

#[tauri::command]
async fn cleanup_preview(targets: Vec<cleanup::Target>) -> Result<cleanup::Preview, String> {
    blocking(move || Ok(cleanup::preview(targets))).await
}

/// Moves items to the Recycle Bin / Trash. Each path is re-validated.
#[tauri::command]
async fn cleanup_execute(paths: Vec<String>) -> Result<cleanup::CleanResult, String> {
    blocking(move || {
        let r = cleanup::execute(paths);
        stats::record("cleanup", r.freed, r.moved as u64);
        Ok(r)
    })
    .await
}

/// Space freed this session and all time, with recent cleanups.
#[tauri::command]
fn cleaning_stats() -> stats::Summary {
    stats::summary()
}

#[tauri::command]
fn check_path(path: String) -> Option<String> {
    match safety::check(std::path::Path::new(&path)) {
        safety::Verdict::Allowed => None,
        safety::Verdict::Protected(r) => Some(r),
    }
}

#[tauri::command]
async fn find_leftovers() -> Result<Vec<leftovers::Leftover>, String> {
    blocking(|| Ok(leftovers::find())).await
}

/// Opens the OS's own cleanup tool (Disk Cleanup on Windows). It does not
/// delete anything by itself; the user chooses in that tool.
#[tauri::command]
fn open_system_cleanup() -> Result<(), String> {
    leftovers::open_system_tool().map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_format_volumes() -> Result<Vec<format::Volume>, String> {
    blocking(|| Ok(format::list())).await
}

/// Erases a volume. The backend re-checks eligibility and the typed
/// confirmation; the OS elevation prompt is always shown.
#[tauri::command]
async fn format_volume(
    mount_point: String,
    file_system: String,
    label: String,
    confirm: String,
) -> Result<(), String> {
    blocking(move || format::format(&mount_point, &file_system, &label, &confirm)).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(ProcState::new())
        .setup(|app| {
            let store = app.path().app_local_data_dir().ok().map(|d| d.join("index"));
            if let Some(dir) = &store {
                index::set_store_dir(dir.clone());
            }
            if let Ok(dir) = app.path().app_local_data_dir() {
                stats::set_file(dir.join("cleaning-stats.json"));
            }
            let handle = app.handle().clone();
            watch::start(move |root| {
                let _ = handle.emit("index-changed", root.to_string());
            }, store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_drives,
            scan_dir,
            scan_progress,
            refresh_index,
            open_full_disk_access,
            largest_files,
            list_processes,
            quit_processes,
            list_caches,
            list_apps,
            app_leftovers,
            remove_app,
            cleanup_preview,
            cleanup_execute,
            cleaning_stats,
            check_path,
            find_leftovers,
            open_system_cleanup,
            list_format_volumes,
            format_volume
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

mod apps;
mod caches;
mod disk;
mod processes;

use processes::ProcState;
use tauri::State;

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
async fn scan_dir(path: String) -> Result<disk::ScanResult, String> {
    blocking(move || disk::scan(&path)).await
}

#[tauri::command]
fn list_processes(state: State<'_, ProcState>) -> processes::ProcSnapshot {
    processes::snapshot(&state)
}

#[tauri::command]
async fn list_caches() -> Result<Vec<caches::CacheInfo>, String> {
    blocking(|| Ok(caches::list())).await
}

#[tauri::command]
async fn list_apps() -> Result<Vec<apps::AppInfo>, String> {
    blocking(|| Ok(apps::list())).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ProcState::new())
        .invoke_handler(tauri::generate_handler![
            list_drives,
            scan_dir,
            list_processes,
            list_caches,
            list_apps
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

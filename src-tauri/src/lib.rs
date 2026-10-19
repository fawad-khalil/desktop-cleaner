mod apps;
mod caches;
mod cleanup;
mod disk;
mod format;
mod leftovers;
mod processes;
mod safety;

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

#[tauri::command]
async fn list_caches() -> Result<Vec<caches::CacheInfo>, String> {
    blocking(|| Ok(caches::list())).await
}

#[tauri::command]
async fn list_apps() -> Result<Vec<apps::AppInfo>, String> {
    blocking(|| Ok(apps::list())).await
}

#[tauri::command]
async fn cleanup_preview(targets: Vec<cleanup::Target>) -> Result<cleanup::Preview, String> {
    blocking(move || Ok(cleanup::preview(targets))).await
}

/// Moves items to the Recycle Bin / Trash. Each path is re-validated.
#[tauri::command]
async fn cleanup_execute(paths: Vec<String>) -> Result<cleanup::CleanResult, String> {
    blocking(move || Ok(cleanup::execute(paths))).await
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
        .invoke_handler(tauri::generate_handler![
            list_drives,
            scan_dir,
            largest_files,
            list_processes,
            list_caches,
            list_apps,
            cleanup_preview,
            cleanup_execute,
            check_path,
            find_leftovers,
            open_system_cleanup,
            list_format_volumes,
            format_volume
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

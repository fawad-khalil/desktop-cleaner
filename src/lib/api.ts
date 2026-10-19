import { invoke } from "@tauri-apps/api/core";

export interface DriveInfo {
  name: string;
  mount_point: string;
  file_system: string;
  kind: string;
  total: number;
  available: number;
  removable: boolean;
}

export interface Entry {
  name: string;
  path: string;
  size: number;
  is_dir: boolean;
  files: number;
  /** Largest items one level down (only on a scan's direct children). */
  children?: Entry[];
}

export interface TypeBucket {
  category: string;
  size: number;
  files: number;
}

export interface ScanResult {
  path: string;
  total: number;
  files: number;
  entries: Entry[];
  types: TypeBucket[];
  ages: TypeBucket[];
  skipped: number;
}

export interface FileItem {
  path: string;
  size: number;
  last_used: number | null;
}

export interface ProcInfo {
  pid: number;
  parent: number | null;
  name: string;
  exe: string | null;
  memory: number;
  cpu: number;
  disk_read: number;
  disk_written: number;
  run_time: number;
  connections: number;
  listening: number;
  /** Friendly name of the app this process belongs to. */
  app: string;
  kind: "app" | "background" | "system";
  /** Runs as the current user. */
  mine: boolean;
  /** Bytes since the process started and current bytes/s; null where the OS doesn't report them. */
  net_rx: number | null;
  net_tx: number | null;
  net_rx_rate: number | null;
  net_tx_rate: number | null;
}

export interface ProcSnapshot {
  processes: ProcInfo[];
  total_memory: number;
  used_memory: number;
  cpu: number;
  cpu_count: number;
  net_rx_rate: number;
  net_tx_rate: number;
  net_available: boolean;
  /** Per-process traffic numbers are available (macOS). */
  net_per_process: boolean;
}

export interface CacheInfo {
  name: string;
  group: string;
  path: string;
  size: number;
  files: number;
}

export interface AppInfo {
  name: string;
  version: string | null;
  publisher: string | null;
  location: string | null;
  size: number | null;
  installed: string | null;
  last_used: number | null;
  /** Why it can't be removed from here; null when it can. */
  blocked: string | null;
}

export interface AppLeftover {
  path: string;
  size: number;
}

export interface RemoveOutcome {
  moved: string[];
  freed: number;
  failed: [string, string][];
  launched: boolean;
  command: string | null;
}

export const api = {
  listDrives: () => invoke<DriveInfo[]>("list_drives"),
  /** Folders inside the last scan are answered from memory unless `fresh`. */
  scanDir: (path: string, fresh = false) => invoke<ScanResult>("scan_dir", { path, fresh }),
  /** Files counted so far by the scan in progress. */
  scanProgress: () => invoke<number>("scan_progress"),
  openFullDiskAccess: () => invoke<void>("open_full_disk_access"),
  largestFiles: (path: string, category: string | null, age: number | null, limit = 100) =>
    invoke<FileItem[]>("largest_files", { path, category, age, limit }),
  listProcesses: () => invoke<ProcSnapshot>("list_processes"),
  /** Asks processes to quit normally; the backend refuses system ones. */
  quitProcesses: (pids: number[]) => invoke<number>("quit_processes", { pids }),
  listCaches: () => invoke<CacheInfo[]>("list_caches"),
  listApps: () => invoke<AppInfo[]>("list_apps"),
  appLeftovers: (location: string) => invoke<AppLeftover[]>("app_leftovers", { location }),
  /** `target`: location on macOS/Linux, display name on Windows. Never deletes permanently. */
  removeApp: (target: string, leftovers: string[]) => invoke<RemoveOutcome>("remove_app", { target, leftovers }),
};

export type Target =
  /** With `min_age_days`, only items nothing inside has been used for that long. */
  | { kind: "contents"; path: string; min_age_days?: number }
  | { kind: "old_files"; path: string; min_age_days: number; min_size: number };

export interface Candidate {
  path: string;
  size: number;
  is_dir: boolean;
  last_used: number | null;
  source: string;
}

export interface Refusal {
  path: string;
  reason: string;
}

export interface Preview {
  items: Candidate[];
  total: number;
  protected: Refusal[];
  protected_count: number;
  truncated: boolean;
}

export interface CleanResult {
  moved: number;
  freed: number;
  failed: Refusal[];
}

export const cleanupApi = {
  preview: (targets: Target[]) => invoke<Preview>("cleanup_preview", { targets }),
  /** Moves items to the Recycle Bin / Trash. Never permanent. */
  execute: (paths: string[]) => invoke<CleanResult>("cleanup_execute", { paths }),
  checkPath: (path: string) => invoke<string | null>("check_path", { path }),
};

export interface Leftover {
  title: string;
  description: string;
  paths: string[];
  size: number;
  action: "trash" | "system_tool" | "command" | "manual";
  command: string | null;
}

export const leftoverApi = {
  find: () => invoke<Leftover[]>("find_leftovers"),
  openSystemCleanup: () => invoke<void>("open_system_cleanup"),
};

export interface Volume {
  mount_point: string;
  device: string;
  label: string;
  file_system: string;
  total: number;
  removable: boolean;
  /** Why it can't be formatted; null when it can. */
  blocked: string | null;
  confirm_text: string;
  file_systems: string[];
}

export const formatApi = {
  list: () => invoke<Volume[]>("list_format_volumes"),
  /** Permanently erases the volume. The backend re-checks everything. */
  format: (mountPoint: string, fileSystem: string, label: string, confirm: string) =>
    invoke<void>("format_volume", { mountPoint, fileSystem, label, confirm }),
};

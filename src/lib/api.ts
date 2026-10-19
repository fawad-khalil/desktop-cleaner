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
}

export const api = {
  listDrives: () => invoke<DriveInfo[]>("list_drives"),
  scanDir: (path: string) => invoke<ScanResult>("scan_dir", { path }),
  largestFiles: (path: string, category: string | null, age: number | null, limit = 100) =>
    invoke<FileItem[]>("largest_files", { path, category, age, limit }),
  listProcesses: () => invoke<ProcSnapshot>("list_processes"),
  listCaches: () => invoke<CacheInfo[]>("list_caches"),
  listApps: () => invoke<AppInfo[]>("list_apps"),
};

export type Target =
  | { kind: "contents"; path: string }
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

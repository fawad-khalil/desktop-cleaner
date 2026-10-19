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
  skipped: number;
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

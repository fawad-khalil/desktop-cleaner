import { invoke } from "@tauri-apps/api/core";

export interface CleanEvent {
  at: number;
  bytes: number;
  items: number;
  kind: "cleanup" | "app";
}

export interface CleaningStats {
  session_bytes: number;
  session_items: number;
  total_bytes: number;
  total_items: number;
  /** When the first cleanup was recorded (epoch seconds). */
  since: number | null;
  /** Newest first. */
  history: CleanEvent[];
}

/** Space freed this session and all time, shared by the sidebar and views. */
export const cleaning = $state<{ stats: CleaningStats | null }>({ stats: null });

export async function refreshCleaningStats() {
  try {
    cleaning.stats = await invoke<CleaningStats>("cleaning_stats");
  } catch {}
}

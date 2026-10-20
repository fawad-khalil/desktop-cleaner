// macOS (Finder, Settings › Storage) counts in powers of 1000; Windows and
// most Linux tools in powers of 1024. Match the system so numbers agree.
const unit = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac") ? 1000 : 1024;

export function bytes(n: number | null | undefined, base = unit): string {
  if (n == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= base && i < units.length - 1) {
    v /= base;
    i++;
  }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

/** Memory sizes: every OS (Activity Monitor included) counts RAM in powers of 1024. */
export const mem = (n: number | null | undefined) => bytes(n, 1024);

export function pct(part: number, whole: number): string {
  if (!whole) return "0%";
  return `${((part / whole) * 100).toFixed(1)}%`;
}

export function ago(epochSecs: number | null | undefined): string {
  if (!epochSecs) return "Unknown";
  const days = Math.floor((Date.now() / 1000 - epochSecs) / 86400);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 30) return `${days} days ago`;
  if (days < 365) return `${Math.floor(days / 30)} months ago`;
  return `${(days / 365).toFixed(1)} years ago`;
}

/** "just now", "5 min ago", "3 h ago", "2 days ago". */
export function since(epochSecs: number | null | undefined, nowMs = Date.now()): string {
  if (!epochSecs) return "never";
  const s = Math.max(0, Math.floor(nowMs / 1000 - epochSecs));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  const d = Math.floor(s / 86400);
  return d === 1 ? "yesterday" : `${d} days ago`;
}

export function daysSince(epochSecs: number | null | undefined): number | null {
  if (!epochSecs) return null;
  return Math.floor((Date.now() / 1000 - epochSecs) / 86400);
}

export const palette = [
  "#4f8cff", "#ff7a59", "#34c38f", "#f7b924", "#a66cff",
  "#2ec4d6", "#ff5c8a", "#8bc34a", "#ff9f43", "#7f8c9a", "#5c6bc0",
];

/** Fixed colours per file type so the storage bar and charts agree. */
export const typeColors: Record<string, string> = {
  "Applications & binaries": "#4f8cff",
  Documents: "#f7b924",
  Images: "#ff7a59",
  Videos: "#a66cff",
  Audio: "#ff5c8a",
  Archives: "#8bc34a",
  "Disk images": "#2ec4d6",
  Code: "#5c6bc0",
  "Logs & temp": "#ff9f43",
  Databases: "#34c38f",
  Other: "#7f8c9a",
};

/** Colour for space we know is used but couldn't attribute to files. */
export const systemColor = "#55606f";
export const freeColor = "var(--border)";

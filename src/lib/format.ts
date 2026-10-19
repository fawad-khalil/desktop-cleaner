export function bytes(n: number | null | undefined): string {
  if (n == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

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

export function daysSince(epochSecs: number | null | undefined): number | null {
  if (!epochSecs) return null;
  return Math.floor((Date.now() / 1000 - epochSecs) / 86400);
}

export const palette = [
  "#4f8cff", "#ff7a59", "#34c38f", "#f7b924", "#a66cff",
  "#2ec4d6", "#ff5c8a", "#8bc34a", "#ff9f43", "#7f8c9a", "#5c6bc0",
];

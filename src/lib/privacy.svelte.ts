import { invoke } from "@tauri-apps/api/core";

export const isMac = navigator.userAgent.includes("Mac");

const NOT_NOW_KEY = "fda-not-now";

function loadNotNow() {
  try {
    return localStorage.getItem(NOT_NOW_KEY) === "1";
  } catch {
    return false;
  }
}

/** macOS Full Disk Access, shared by the card and the views. `granted` is
 * null until checked; it's always true off macOS. */
export const access = $state<{ granted: boolean | null; notNow: boolean }>({ granted: null, notNow: loadNotNow() });

/** True once the user has granted access or said "Not now", so work that
 * would read protected folders can start without competing with the card. */
export function decided() {
  return access.granted === true || (access.granted === false && access.notNow);
}

/** Checks without prompting; `recheck` after a visit to System Settings. */
export async function checkAccess(recheck = false) {
  try {
    access.granted = await invoke<boolean>("full_disk_access", { recheck });
  } catch {
    access.granted = true;
  }
}

export function notNow() {
  access.notNow = true;
  try {
    localStorage.setItem(NOT_NOW_KEY, "1");
  } catch {}
}

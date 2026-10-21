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

export interface AccessInfo {
  granted: boolean;
  /** Running outside an app bundle (e.g. `tauri dev`): macOS applies this app's permission instead. */
  host: string | null;
  /** The build's signature isn't sealed to its bundle, so the Settings switch can't apply to it. */
  unsealed: boolean;
}

/** macOS Full Disk Access, shared by the card and the views. `granted` is
 * null until checked; it's always true off macOS. */
export const access = $state<{ granted: boolean | null; notNow: boolean; host: string | null; unsealed: boolean }>({
  granted: null,
  notNow: loadNotNow(),
  host: null,
  unsealed: false,
});

/** True once the user has granted access or chose "Continue without", so work that
 * would read protected folders can start without competing with the card. */
export function decided() {
  return access.granted === true || (access.granted === false && access.notNow);
}

/** Checks without prompting; `recheck` after a visit to System Settings. */
export async function checkAccess(recheck = false) {
  try {
    const a = await invoke<AccessInfo>("full_disk_access", { recheck });
    access.host = a.host;
    access.unsealed = a.unsealed;
    access.granted = a.granted;
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

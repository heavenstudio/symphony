// What this device keeps for the page's project in the browser's storage: the projects of the
// machine's one server share its origin, so every key is the project's own (`stored`). Storage can
// be missing or refuse a write (a private window, a full quota): a read then finds nothing and a
// write keeps nothing.

import { project, stored } from "./project";

/** The value this device kept under `key` for the page's project, or none. */
export function load(key: string): string | null {
  try {
    return localStorage.getItem(stored(project(), key));
  } catch {
    return null;
  }
}

/** Keep `value` under `key` for the page's project; best effort. */
export function save(key: string, value: string): void {
  try {
    localStorage.setItem(stored(project(), key), value);
  } catch {
    // Kept nowhere, then: the page works without it.
  }
}

/** The tab's own value under `key` for the page's project (sessionStorage), or none. */
export function loadForTab(key: string): string | null {
  try {
    return sessionStorage.getItem(stored(project(), key));
  } catch {
    return null;
  }
}

/** Keep `value` under `key` for this tab and the page's project; whether it was kept. */
export function saveForTab(key: string, value: string): boolean {
  try {
    sessionStorage.setItem(stored(project(), key), value);
    return true;
  } catch {
    return false;
  }
}

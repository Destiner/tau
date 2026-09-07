/*
 * Admin mode: Tau's diagnostics are off unless the user has asked for them.
 * It gates telemetry recording and the issue reporter together, and the
 * native side owns the persisted setting (see `src-tauri/src/admin.rs`), so
 * a launch decides before it records anything rather than after the
 * frontend has mounted.
 */
import { invoke } from '@tauri-apps/api/core';
import { readonly, ref, type Ref } from 'vue';

import { invokeTraced, setTelemetryEnabled } from './telemetry';

const enabled = ref(false);

/** Whether this run is in admin mode. Read-only: `loadAdminMode` and
 * `setAdminMode` are the only ways in, so telemetry can never be left
 * recording behind a switch that reads as off. */
const adminMode: Readonly<Ref<boolean>> = readonly(enabled);

/** Reads the persisted setting and applies it to this run. Called once at
 * startup, before Vue mounts, so an admin run starts recording as early as
 * it can. Outside a Tauri webview — a browser test, `vite dev` in a tab —
 * `invoke` throws and admin mode stays off, which is the same answer a
 * fresh install gives. */
async function loadAdminMode(): Promise<void> {
  let stored: boolean;
  try {
    // The one Tau command deliberately left untraced: it runs to decide
    // whether telemetry may record at all, so its own span could only ever
    // be dropped.
    stored = await invoke<boolean>('read_admin_mode');
  } catch {
    stored = false;
  }
  apply(stored);
}

/** Applies the switch to this run and asks the native side to persist it
 * and to start or stop its own recording. A failed write is swallowed: the
 * switch still holds for this run, which is what the user just asked for,
 * and telemetry has no business surfacing an error over it. */
async function setAdminMode(next: boolean): Promise<void> {
  apply(next);
  try {
    await invokeTraced('set_admin_mode', { enabled: next });
  } catch {
    return;
  }
}

function toggleAdminMode(): Promise<void> {
  return setAdminMode(!enabled.value);
}

function apply(next: boolean): void {
  enabled.value = next;
  setTelemetryEnabled(next);
}

export { adminMode, loadAdminMode, setAdminMode, toggleAdminMode };

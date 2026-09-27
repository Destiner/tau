import { invoke } from '@tauri-apps/api/core';
import { readonly, ref, type Ref } from 'vue';

import { invokeTraced, setTelemetryEnabled } from './telemetry';

const enabled = ref(false);

/** Whether this run is in admin mode. Read-only: `loadAdminMode` and
 * `setAdminMode` are the only ways in, so telemetry can never be left
 * recording behind a switch that reads as off. */
const adminMode: Readonly<Ref<boolean>> = readonly(enabled);

async function loadAdminMode(): Promise<void> {
  let stored: boolean;
  try {
    stored = await invoke<boolean>('read_admin_mode');
  } catch {
    stored = false;
  }
  apply(stored);
}

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

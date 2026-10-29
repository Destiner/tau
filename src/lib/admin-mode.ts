import { invoke } from '@tauri-apps/api/core';
import { readonly, ref, type Ref } from 'vue';

import { issueReportingApi, setCloudAdminMode } from './cloud-connections';
import { invokeTraced } from './telemetry';

const enabled = ref(false);
const adminMode: Readonly<Ref<boolean>> = readonly(enabled);
let revision = 0;
let pendingWrite: Promise<void> = Promise.resolve();

async function refreshTelemetry(current: number): Promise<void> {
  if (current !== revision || !enabled.value) return;
  try {
    await issueReportingApi.readConnections();
  } catch {
    // Connection errors belong in the reporter; diagnostics stay disabled.
  }
}

async function loadAdminMode(): Promise<void> {
  const current = ++revision;
  let stored = false;
  try {
    stored = (await invoke<boolean>('read_admin_mode')) === true;
  } catch {
    // A failed read must never enable diagnostics.
  }
  if (current !== revision) return;
  apply(stored);
  await refreshTelemetry(current);
}

function setAdminMode(next: boolean): Promise<void> {
  const current = ++revision;
  apply(next);
  // Preserve toggle order even when a native preferences/Keychain operation is slow.
  pendingWrite = pendingWrite.then(async () => {
    try {
      await invokeTraced('set_admin_mode', { enabled: next });
    } catch {
      // The native runtime gate changes before preference persistence.
    }
    await refreshTelemetry(current);
  });
  return pendingWrite;
}

function toggleAdminMode(): Promise<void> {
  return setAdminMode(!enabled.value);
}

function apply(next: boolean): void {
  enabled.value = next;
  setCloudAdminMode(next);
}

export { adminMode, loadAdminMode, setAdminMode, toggleAdminMode };

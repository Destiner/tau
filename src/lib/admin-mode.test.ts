import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => undefined),
}));

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue(undefined);
  vi.resetModules();
});

/** Loads both halves of the switch from one module registry, so the admin
 * module under test is the one the telemetry adapter is wired to. */
async function loadAdmin(): Promise<{
  admin: typeof import('./admin-mode');
  telemetry: typeof import('./telemetry');
}> {
  return {
    admin: await import('./admin-mode'),
    telemetry: await import('./telemetry'),
  };
}

function ingestCalls(): unknown[] {
  return mockInvoke.mock.calls.filter(([name]) => name === 'ingest_telemetry');
}

describe('admin mode', () => {
  it('is off before anything has read the setting, and records no telemetry', async () => {
    const { admin, telemetry } = await loadAdmin();

    expect(admin.adminMode.value).toBe(false);
    telemetry.startCommandSpan('load_workspace').end();
    await telemetry.flushTelemetry();

    expect(ingestCalls()).toHaveLength(0);
  });

  it('stays off when the persisted setting is off', async () => {
    mockInvoke.mockResolvedValue(false);
    const { admin, telemetry } = await loadAdmin();

    await admin.loadAdminMode();

    expect(mockInvoke).toHaveBeenCalledWith('read_admin_mode');
    expect(admin.adminMode.value).toBe(false);
    telemetry.startCommandSpan('load_workspace').end();
    await telemetry.flushTelemetry();
    expect(ingestCalls()).toHaveLength(0);
  });

  it('starts recording once the persisted setting says the run is an admin one', async () => {
    mockInvoke.mockResolvedValue(true);
    const { admin, telemetry } = await loadAdmin();

    await admin.loadAdminMode();

    expect(admin.adminMode.value).toBe(true);
    telemetry.startCommandSpan('load_workspace').end();
    await telemetry.flushTelemetry();
    expect(ingestCalls()).toHaveLength(1);
  });

  it('stays off when the setting cannot be read at all', async () => {
    mockInvoke.mockRejectedValue(new Error('no tauri here'));
    const { admin } = await loadAdmin();

    await admin.loadAdminMode();

    expect(admin.adminMode.value).toBe(false);
  });

  it('toggles this run and persists the new setting', async () => {
    const { admin, telemetry } = await loadAdmin();

    await admin.toggleAdminMode();

    expect(admin.adminMode.value).toBe(true);
    expect(mockInvoke).toHaveBeenCalledWith(
      'set_admin_mode',
      expect.objectContaining({ enabled: true }),
    );
    telemetry.startCommandSpan('load_workspace').end();
    await telemetry.flushTelemetry();
    expect(ingestCalls()).toHaveLength(1);

    await admin.toggleAdminMode();
    expect(admin.adminMode.value).toBe(false);
    expect(mockInvoke).toHaveBeenCalledWith(
      'set_admin_mode',
      expect.objectContaining({ enabled: false }),
    );
  });

  it('holds the switch for this run even when it cannot be persisted', async () => {
    mockInvoke.mockRejectedValue(new Error('read-only disk'));
    const { admin } = await loadAdmin();

    await admin.setAdminMode(true);

    expect(admin.adminMode.value).toBe(true);
  });

  it('drops what telemetry had queued when admin mode is turned back off', async () => {
    const { admin, telemetry } = await loadAdmin();
    await admin.setAdminMode(true);
    telemetry.startCommandSpan('load_workspace').end();

    await admin.setAdminMode(false);
    await telemetry.flushTelemetry();

    expect(ingestCalls()).toHaveLength(0);
  });
});

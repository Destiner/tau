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
  it('remains off without a persisted admin setting or if reading it fails', async () => {
    const { admin, telemetry } = await loadAdmin();

    expect(admin.adminMode.value).toBe(false);
    telemetry.startCommandSpan('load_workspace').end();
    await telemetry.flushTelemetry();

    expect(ingestCalls()).toHaveLength(0);
    mockInvoke.mockResolvedValue(false);

    await admin.loadAdminMode();

    expect(mockInvoke).toHaveBeenCalledWith('read_admin_mode');
    expect(admin.adminMode.value).toBe(false);
    telemetry.startCommandSpan('load_workspace').end();
    await telemetry.flushTelemetry();
    expect(ingestCalls()).toHaveLength(0);
    mockInvoke.mockRejectedValue(new Error('no tauri here'));
    await admin.loadAdminMode();
    expect(admin.adminMode.value).toBe(false);
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

  it('toggles this run, persisting when possible but holding the switch on failure', async () => {
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
    mockInvoke.mockRejectedValue(new Error('read-only disk'));
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

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

describe('admin mode', () => {
  it('remains off without a persisted admin setting or if reading it fails', async () => {
    const admin = await import('./admin-mode');

    expect(admin.adminMode.value).toBe(false);
    mockInvoke.mockResolvedValue(false);

    await admin.loadAdminMode();

    expect(mockInvoke).toHaveBeenCalledWith('read_admin_mode');
    expect(admin.adminMode.value).toBe(false);
    mockInvoke.mockRejectedValue(new Error('no tauri here'));
    await admin.loadAdminMode();
    expect(admin.adminMode.value).toBe(false);
  });

  it('restores admin mode from the persisted setting', async () => {
    mockInvoke.mockResolvedValue(true);
    const admin = await import('./admin-mode');

    await admin.loadAdminMode();

    expect(admin.adminMode.value).toBe(true);
  });

  it('toggles this run, persisting when possible but holding the switch on failure', async () => {
    const admin = await import('./admin-mode');

    await admin.toggleAdminMode();

    expect(admin.adminMode.value).toBe(true);
    expect(mockInvoke).toHaveBeenCalledWith(
      'set_admin_mode',
      expect.objectContaining({ enabled: true }),
    );

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
});

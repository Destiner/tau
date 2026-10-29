import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { CloudConnections } from './cloud-types';

const { invoke, enable } = vi.hoisted(() => ({
  invoke: vi.fn(),
  enable: vi.fn(),
}));
vi.mock('./telemetry', () => ({
  invokeTraced: invoke,
  setTelemetryEnabled: enable,
}));

const disconnected: CloudConnections = {
  linear: null,
  grafana: null,
  telemetryEnabled: false,
};
const connected: CloudConnections = {
  ...disconnected,
  grafana: { endpoint: 'https://example.invalid/otlp', instanceId: '1' },
  telemetryEnabled: true,
};

beforeEach(() => {
  vi.resetModules();
  invoke.mockReset();
  enable.mockReset();
});

describe('cloud connection gating', () => {
  it('rejects operations before invoking native commands while admin is off', async () => {
    const { issueReportingApi: api } = await import('./cloud-connections');
    await expect(api.readConnections()).rejects.toEqual({
      code: 'admin_required',
    });
    await expect(api.validateLinear('fake-key')).rejects.toEqual({
      code: 'admin_required',
    });
    expect(invoke).not.toHaveBeenCalled();
  });

  it('requires both admin mode and a confirmed Grafana destination', async () => {
    const { issueReportingApi: api, setCloudAdminMode } =
      await import('./cloud-connections');
    setCloudAdminMode(true);
    expect(enable).toHaveBeenLastCalledWith(false);
    invoke.mockResolvedValue({ ...disconnected, telemetryEnabled: true });
    await api.readConnections();
    expect(enable).toHaveBeenLastCalledWith(false);
    invoke.mockResolvedValue(connected);
    await api.readConnections();
    expect(enable).toHaveBeenLastCalledWith(true);
    setCloudAdminMode(false);
    expect(enable).toHaveBeenLastCalledWith(false);
  });

  it('rejects a response from before admin was disabled and re-enabled', async () => {
    const { issueReportingApi: api, setCloudAdminMode } =
      await import('./cloud-connections');
    setCloudAdminMode(true);
    let resolve!: (value: CloudConnections) => void;
    invoke.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const pending = api.readConnections();
    setCloudAdminMode(false);
    setCloudAdminMode(true);
    resolve(connected);
    await expect(pending).rejects.toEqual({ code: 'admin_required' });
    expect(enable).not.toHaveBeenCalledWith(true);
  });

  it('does not reactivate telemetry from a stale read after disconnect', async () => {
    const { issueReportingApi: api, setCloudAdminMode } =
      await import('./cloud-connections');
    setCloudAdminMode(true);
    let resolve!: (value: CloudConnections) => void;
    invoke.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const pending = api.readConnections();
    invoke.mockResolvedValue(disconnected);
    await api.disconnect('grafana');
    resolve(connected);
    await pending;
    expect(enable).not.toHaveBeenCalledWith(true);
    expect(invoke).toHaveBeenLastCalledWith('disconnect_cloud_connection', {
      provider: 'grafana',
    });
  });

  it('recognizes only reviewed native error categories', async () => {
    const { cloudErrorCode } = await import('./cloud-connections');
    expect(cloudErrorCode({ code: 'authentication' })).toBe('authentication');
    expect(cloudErrorCode(new Error('raw response'))).toBeUndefined();
    expect(
      cloudErrorCode({ code: 'credential-bearing provider error' }),
    ).toBeUndefined();
    expect(cloudErrorCode('authentication')).toBeUndefined();
  });
});

import { afterEach, describe, expect, it, vi } from 'vitest';

import type { CloudConnections, IssueReportingApi } from '../lib/cloud-types';

import { createIssueDraft, createIssueReporter } from './useIssueReporter';

const destination = {
  teamId: 'team',
  teamName: 'Personal',
  projectId: 'tau',
  projectName: 'Tau',
};
function fake(configured = true): IssueReportingApi {
  let connections: CloudConnections = {
    linear: configured ? destination : null,
    grafana: null,
    telemetryEnabled: false,
  };
  return {
    readConnections: vi.fn(async () => ({ ...connections })),
    validateLinear: vi.fn(async () => ({
      teams: [
        {
          id: 'team',
          name: 'Personal',
          projects: [{ id: 'tau', name: 'Tau' }],
        },
      ],
    })),
    saveLinear: vi.fn(async () => {
      connections = { ...connections, linear: destination };
      return connections;
    }),
    saveGrafana: vi.fn(async () => {
      connections = {
        ...connections,
        grafana: { endpoint: 'https://example.invalid/otlp', instanceId: '1' },
        telemetryEnabled: true,
      };
      return connections;
    }),
    disconnect: vi.fn(async (provider) => {
      connections = {
        ...connections,
        [provider]: null,
        telemetryEnabled:
          provider === 'grafana' ? false : connections.telemetryEnabled,
      };
      return connections;
    }),
    submitReport: vi.fn(async (report) => ({
      id: report.reportId,
      identifier: 'TAU-1',
      url: 'https://linear.app/issue/TAU-1',
    })),
  };
}

afterEach(() => vi.useRealTimers());

describe('cloud issue reporting', () => {
  it('sets up Linear step by step and permits reporting after Grafana fails', async () => {
    const api = fake(false);
    const model = createIssueReporter(
      api,
      () => 'session-1',
      createIssueDraft(),
    );
    await model.load();
    expect(model.view.value).toBe('linear');
    model.linearKey.value = 'fake-key';
    await model.validateLinear();
    expect(model.view.value).toBe('destination');
    expect(model.teamId.value).toBe('team');
    await model.saveLinear();
    expect(api.saveLinear).toHaveBeenCalledWith({
      apiKey: 'fake-key',
      teamId: 'team',
    });
    expect(model.linearKey.value).toBe('');
    expect(model.view.value).toBe('grafana');
    model.endpoint.value = 'https://example.invalid/otlp';
    model.instanceId.value = '1';
    model.token.value = 'fake-token';
    vi.mocked(api.saveGrafana).mockRejectedValue({ code: 'network' });
    await model.saveGrafana();
    expect(model.errors.grafana).toBe('Could not reach Grafana. Try again.');
    model.showReport();
    expect(model.view.value).toBe('report');
    model.description.value = 'The sidebar stopped responding.';
    await model.submitReport();
    expect(api.submitReport).toHaveBeenCalledWith(
      expect.objectContaining({
        description: 'The sidebar stopped responding.',
      }),
    );
    expect(vi.mocked(api.submitReport).mock.calls[0]?.[0]).not.toHaveProperty(
      'sessionId',
    );
    expect(model.description.value).toBe('');
    model.dispose();
  });

  it('keeps the same report identity and original opted-in session on retry', async () => {
    const api = fake();
    let session = 'original';
    const model = createIssueReporter(api, () => session, createIssueDraft());
    await model.load();
    model.description.value = 'A report';
    model.includeSession.value = true;
    vi.mocked(api.submitReport).mockRejectedValue({ code: 'network' });
    await model.submitReport();
    const first = vi.mocked(api.submitReport).mock.calls[0]![0];
    expect(first.sessionId).toBe('original');
    expect(first.reportId).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
    expect(first.timeUnixNano).toMatch(/^[1-9][0-9]+$/);
    session = 'different';
    model.setOpen(false);
    model.openReporter();
    await model.submitReport();
    expect(vi.mocked(api.submitReport).mock.calls[1]![0]).toEqual(first);
    model.description.value = 'Edited report';
    await model.submitReport();
    expect(vi.mocked(api.submitReport).mock.calls[2]![0].reportId).not.toBe(
      first.reportId,
    );
    expect(vi.mocked(api.submitReport).mock.calls[2]![0].sessionId).toBe(
      'different',
    );
    model.dispose();
  });

  it('reconnects revoked credentials without losing the report or destination', async () => {
    const api = fake();
    const model = createIssueReporter(api, () => 's', createIssueDraft());
    await model.load();
    model.description.value = 'Keep this report';
    vi.mocked(api.submitReport).mockRejectedValue({ code: 'authentication' });
    await model.submitReport();
    expect(model.view.value).toBe('linear');
    expect(model.reconnecting.value).toBe(true);
    model.linearKey.value = 'replacement';
    await model.validateLinear();
    expect(api.saveLinear).toHaveBeenCalledWith({
      apiKey: 'replacement',
      teamId: 'team',
      projectId: 'tau',
    });
    expect(model.view.value).toBe('report');
    expect(model.description.value).toBe('Keep this report');
    model.dispose();
  });

  it('disconnects only the selected provider and clears its entry fields', async () => {
    const api = fake();
    const model = createIssueReporter(api, () => undefined, createIssueDraft());
    await model.load();
    model.description.value = 'Keep me';
    model.endpoint.value = 'https://example.invalid/otlp';
    model.instanceId.value = '1';
    model.token.value = 'fake';
    await model.saveGrafana();
    await model.disconnect('grafana');
    expect(model.connections.value.linear).toEqual(destination);
    expect(model.endpoint.value).toBe('');
    expect(model.instanceId.value).toBe('');
    expect(model.token.value).toBe('');
    await model.disconnect('linear');
    expect(model.connections.value.linear).toBeNull();
    expect(model.description.value).toBe('Keep me');
    model.dispose();
  });

  it('does not let an abandoned submission clear a draft used by a replacement reporter', async () => {
    const api = fake();
    const draft = createIssueDraft();
    const old = createIssueReporter(api, () => undefined, draft);
    await old.load();
    old.description.value = 'A valuable draft';
    let finish!: (receipt: {
      id: string;
      identifier: string;
      url: string;
    }) => void;
    vi.mocked(api.submitReport).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const pending = old.submitReport();
    expect(old.busy.value).toBe('send');
    old.dispose();
    const replacement = createIssueReporter(api, () => undefined, draft);
    await replacement.load();
    finish({
      id: 'irrelevant',
      identifier: 'TAU-1',
      url: 'https://linear.app',
    });
    await pending;
    expect(replacement.description.value).toBe('A valuable draft');
    await replacement.submitReport();
    expect(replacement.description.value).toBe('');
    replacement.dispose();
  });

  it('bounds requests, prevents duplicate submits, and never renders raw failures', async () => {
    vi.useFakeTimers();
    const api = fake();
    const model = createIssueReporter(api, () => undefined, createIssueDraft());
    await model.load();
    model.description.value = 'Report';
    vi.mocked(api.submitReport).mockImplementation(() => new Promise(() => {}));
    const pending = model.submitReport();
    await model.submitReport();
    expect(api.submitReport).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(35_000);
    await pending;
    expect(model.busy.value).toBeNull();
    expect(model.description.value).toBe('Report');
    vi.mocked(api.submitReport).mockRejectedValue(
      new Error('credential-bearing provider response'),
    );
    await model.submitReport();
    expect(model.errors.send).toBe('Could not send the report. Try again.');
    model.dispose();
  });
});

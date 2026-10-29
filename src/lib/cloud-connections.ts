import type {
  CloudConnections,
  CloudErrorCode,
  IssueReportingApi,
  LinearOptions,
} from './cloud-types';
import { invokeTraced, setTelemetryEnabled } from './telemetry';

let adminEnabled = false;
let epoch = 0;
let snapshotRevision = 0;

function setCloudAdminMode(enabled: boolean): void {
  adminEnabled = enabled;
  epoch++;
  snapshotRevision++;
  setTelemetryEnabled(false);
}

function cloudErrorCode(value: unknown): CloudErrorCode | undefined {
  if (!value || typeof value !== 'object' || !('code' in value)) return;
  switch (value.code) {
    case 'admin_required':
    case 'authentication':
    case 'network':
    case 'invalid':
    case 'storage':
    case 'not_configured':
    case 'unavailable':
      return value.code;
    default:
      return;
  }
}

function requireAdmin(): number {
  if (!adminEnabled) throw { code: 'admin_required' };
  return epoch;
}

function checkEpoch(started: number): void {
  if (!adminEnabled || started !== epoch) throw { code: 'admin_required' };
}

async function snapshotRequest(
  request: () => Promise<CloudConnections>,
): Promise<CloudConnections> {
  const started = requireAdmin();
  const revision = ++snapshotRevision;
  const snapshot = await request();
  checkEpoch(started);
  if (!snapshot || typeof snapshot.telemetryEnabled !== 'boolean') {
    throw { code: 'unavailable' };
  }
  if (revision === snapshotRevision) {
    setTelemetryEnabled(Boolean(snapshot.grafana && snapshot.telemetryEnabled));
  }
  return snapshot;
}

const issueReportingApi: IssueReportingApi = {
  readConnections: () =>
    snapshotRequest(() => invokeTraced('read_cloud_connections')),
  async validateLinear(apiKey) {
    const started = requireAdmin();
    const options = await invokeTraced<LinearOptions>(
      'validate_linear_connection',
      { apiKey },
    );
    checkEpoch(started);
    return options;
  },
  saveLinear: (setup) =>
    snapshotRequest(() => invokeTraced('save_linear_connection', { ...setup })),
  saveGrafana: (setup) =>
    snapshotRequest(() =>
      invokeTraced('save_grafana_connection', { ...setup }),
    ),
  disconnect: (provider) => {
    if (provider === 'grafana') setTelemetryEnabled(false);
    return snapshotRequest(() =>
      invokeTraced('disconnect_cloud_connection', { provider }),
    );
  },
  async submitReport(report) {
    const started = requireAdmin();
    const receipt = await invokeTraced<
      Awaited<ReturnType<IssueReportingApi['submitReport']>>
    >('submit_issue_report', { ...report });
    checkEpoch(started);
    return receipt;
  },
};

export { issueReportingApi, setCloudAdminMode, cloudErrorCode };

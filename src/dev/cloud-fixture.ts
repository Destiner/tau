import type {
  CloudConnections,
  IssueReceipt,
  IssueReportInput,
  IssueReportingApi,
} from '../lib/cloud-types';

const fixtureTeam = {
  id: 'fixture-team',
  name: 'Personal',
  projects: [
    { id: 'fixture-tau', name: 'Tau' },
    { id: 'fixture-isle', name: 'Isle' },
  ],
};

interface FixtureOptions {
  configured?: boolean;
  delay?: number;
  onReport?: (report: IssueReportInput) => void;
}

function createCloudFixture(options: FixtureOptions = {}): IssueReportingApi {
  let connections: CloudConnections = {
    linear: options.configured
      ? {
          teamId: fixtureTeam.id,
          teamName: fixtureTeam.name,
          projectId: 'fixture-tau',
          projectName: 'Tau',
        }
      : null,
    grafana: null,
    telemetryEnabled: false,
  };
  const reports = new Map<string, IssueReceipt>();
  async function wait(): Promise<void> {
    if (options.delay)
      await new Promise((resolve) => setTimeout(resolve, options.delay));
  }
  function validate(value: string): void {
    if (!value?.trim()) throw { code: 'invalid' };
    if (value === 'bad') throw { code: 'authentication' };
    if (value === 'offline') throw { code: 'network' };
  }
  const api: IssueReportingApi = {
    async readConnections() {
      await wait();
      return structuredClone(connections);
    },
    async validateLinear(apiKey) {
      await wait();
      validate(apiKey);
      return { teams: [structuredClone(fixtureTeam)] };
    },
    async saveLinear(setup) {
      await wait();
      validate(setup.apiKey);
      const project = fixtureTeam.projects.find(
        (item) => item.id === setup.projectId,
      );
      connections = {
        ...connections,
        linear: {
          teamId: fixtureTeam.id,
          teamName: fixtureTeam.name,
          projectId: project?.id ?? null,
          projectName: project?.name ?? null,
        },
      };
      return structuredClone(connections);
    },
    async saveGrafana(setup) {
      await wait();
      validate(setup.token);
      if (!setup.endpoint.startsWith('https://') || !setup.instanceId.trim())
        throw { code: 'invalid' };
      connections = {
        ...connections,
        grafana: { endpoint: setup.endpoint, instanceId: setup.instanceId },
        telemetryEnabled: true,
      };
      return structuredClone(connections);
    },
    async disconnect(provider) {
      await wait();
      connections = {
        ...connections,
        [provider]: null,
        telemetryEnabled:
          provider === 'grafana' ? false : connections.telemetryEnabled,
      };
      return structuredClone(connections);
    },
    async submitReport(report) {
      await wait();
      if (!connections.linear) throw { code: 'not_configured' };
      if (report.description === 'fail') throw { code: 'unavailable' };
      if (report.description === 'reconnect') throw { code: 'authentication' };
      options.onReport?.(report);
      const receipt = reports.get(report.reportId) ?? {
        id: report.reportId,
        identifier: 'TAU-1',
        url: 'https://linear.example.invalid/TAU-1',
      };
      reports.set(report.reportId, receipt);
      return receipt;
    },
  };
  return api;
}

function createCloudCommandHandler(
  configured = false,
): (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown> | undefined {
  const api = createCloudFixture({ configured });
  let admin = false;
  return (command, args) => {
    if (command === 'read_admin_mode') return Promise.resolve(admin);
    if (command === 'set_admin_mode') {
      admin = args.enabled === true;
      return Promise.resolve(null);
    }
    const handlers: Record<string, () => Promise<unknown>> = {
      read_cloud_connections: api.readConnections,
      validate_linear_connection: () =>
        api.validateLinear(args.apiKey as string),
      save_linear_connection: () =>
        api.saveLinear({
          apiKey: args.apiKey as string,
          teamId: args.teamId as string,
          projectId: args.projectId as string | undefined,
        }),
      save_grafana_connection: () =>
        api.saveGrafana({
          endpoint: args.endpoint as string,
          instanceId: args.instanceId as string,
          token: args.token as string,
        }),
      disconnect_cloud_connection: () =>
        api.disconnect(args.provider as 'linear' | 'grafana'),
      submit_issue_report: () =>
        api.submitReport(args as unknown as IssueReportInput),
    };
    const handler = handlers[command];
    if (!handler) return;
    if (!admin) return Promise.reject({ code: 'admin_required' });
    return handler();
  };
}

export { createCloudFixture, createCloudCommandHandler };

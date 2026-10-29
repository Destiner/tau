interface LinearConnection {
  teamId: string;
  teamName: string;
  projectId: string | null;
  projectName: string | null;
}

interface GrafanaConnection {
  endpoint: string;
  instanceId: string;
}

interface CloudConnections {
  linear: LinearConnection | null;
  grafana: GrafanaConnection | null;
  telemetryEnabled: boolean;
}

interface LinearProject {
  id: string;
  name: string;
}

interface LinearTeam {
  id: string;
  name: string;
  projects: LinearProject[];
}

interface LinearOptions {
  teams: LinearTeam[];
}

interface LinearSetup {
  apiKey: string;
  teamId: string;
  projectId?: string;
}

interface GrafanaSetup {
  endpoint: string;
  instanceId: string;
  token: string;
}

type CloudProvider = 'linear' | 'grafana';
type CloudErrorCode =
  | 'admin_required'
  | 'authentication'
  | 'network'
  | 'invalid'
  | 'storage'
  | 'not_configured'
  | 'unavailable';

interface IssueReportInput {
  description: string;
  sessionId?: string;
  reportId: string;
  timeUnixNano: string;
}

interface IssueReceipt {
  id: string;
  identifier: string;
  url: string;
}

interface IssueReportingApi {
  readConnections: () => Promise<CloudConnections>;
  validateLinear: (apiKey: string) => Promise<LinearOptions>;
  saveLinear: (setup: LinearSetup) => Promise<CloudConnections>;
  saveGrafana: (setup: GrafanaSetup) => Promise<CloudConnections>;
  disconnect: (provider: CloudProvider) => Promise<CloudConnections>;
  submitReport: (report: IssueReportInput) => Promise<IssueReceipt>;
}

export type {
  LinearConnection,
  GrafanaConnection,
  CloudConnections,
  LinearProject,
  LinearTeam,
  LinearOptions,
  LinearSetup,
  GrafanaSetup,
  CloudProvider,
  CloudErrorCode,
  IssueReportInput,
  IssueReceipt,
  IssueReportingApi,
};

import { computed, reactive, ref, toRef, watch } from 'vue';

import { cloudErrorCode } from '../lib/cloud-connections';
import type {
  CloudConnections,
  CloudErrorCode,
  CloudProvider,
  IssueReportInput,
  IssueReportingApi,
  LinearConnection,
  LinearTeam,
} from '../lib/cloud-types';

interface IssueDraft {
  description: string;
  includeSession: boolean;
  attempt?: IssueReportInput;
}

type View =
  | 'loading'
  | 'load-error'
  | 'linear'
  | 'destination'
  | 'grafana'
  | 'report'
  | 'connections';
type Operation =
  | 'load'
  | 'linear'
  | 'destination'
  | 'grafana'
  | 'send'
  | 'disconnect-linear'
  | 'disconnect-grafana';
type Outcome<T> = { ok: true; value: T } | { ok: false; code?: CloudErrorCode };

function createIssueDraft(): IssueDraft {
  return reactive({ description: '', includeSession: false });
}

const sharedDraft = createIssueDraft();
const NO_PROJECT = '__none__';
const REQUEST_TIMEOUT_MS = 35_000;

function errorCopy(operation: Operation, code?: CloudErrorCode): string {
  const service = operation.includes('grafana') ? 'Grafana' : 'Linear';
  if (code === 'admin_required') return 'Enable admin mode to continue.';
  if (code === 'storage')
    return 'Could not access the saved connection. Reconnect and try again.';
  if (code === 'authentication')
    return `Check the ${service} credentials and permissions.`;
  if (code === 'network') return `Could not reach ${service}. Try again.`;
  if (code === 'invalid' && operation === 'grafana')
    return 'Check the HTTPS OTLP endpoint, instance ID, and token.';
  if (code === 'invalid' && operation === 'destination')
    return 'Choose an available team and project.';
  if (operation === 'load') return 'Could not load connections. Try again.';
  if (operation === 'send') return 'Could not send the report. Try again.';
  if (operation.startsWith('disconnect'))
    return `Could not disconnect ${service}. Try again.`;
  if (operation === 'destination')
    return 'Could not save the destination. Try again.';
  return `Could not connect ${service}. Try again.`;
}

// Drafts survive closing the reporter/admin mode, but are never written to disk.
// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function createIssueReporter(
  api: IssueReportingApi,
  sessionId: () => string | undefined,
  draft: IssueDraft = sharedDraft,
) {
  const open = ref(false);
  const view = ref<View>('loading');
  const connections = ref<CloudConnections>({
    linear: null,
    grafana: null,
    telemetryEnabled: false,
  });
  const busy = ref<Operation | null>(null);
  const showSpinner = ref(false);
  const errors = reactive<Record<Operation, string>>({
    load: '',
    linear: '',
    destination: '',
    grafana: '',
    send: '',
    'disconnect-linear': '',
    'disconnect-grafana': '',
  });
  const linearKey = ref('');
  const teams = ref<LinearTeam[]>([]);
  const teamId = ref('');
  const projectId = ref(NO_PROJECT);
  const validated = ref(false);
  const endpoint = ref('');
  const instanceId = ref('');
  const token = ref('');
  const reconnecting = ref(false);
  let previousDestination: LinearConnection | null = null;
  let loaded = false;
  let disposed = false;
  let generation = 0;
  let spinnerTimer: ReturnType<typeof setTimeout> | undefined;
  let requestTimer: ReturnType<typeof setTimeout> | undefined;

  const description = toRef(draft, 'description');
  const includeSession = toRef(draft, 'includeSession');
  const selectedTeam = computed(() =>
    teams.value.find((team) => team.id === teamId.value),
  );
  const projectOptions = computed(() => [
    { value: NO_PROJECT, label: 'No Project' },
    ...(selectedTeam.value?.projects ?? []).map((project) => ({
      value: project.id,
      label: project.name,
    })),
  ]);
  const teamOptions = computed(() =>
    teams.value.map((team) => ({ value: team.id, label: team.name })),
  );
  const canValidate = computed(() => !!linearKey.value.trim() && !busy.value);
  const canSaveDestination = computed(
    () => validated.value && !!selectedTeam.value && !busy.value,
  );
  const canSaveGrafana = computed(
    () =>
      !!endpoint.value.trim() &&
      !!instanceId.value.trim() &&
      !!token.value.trim() &&
      !busy.value,
  );
  const canSubmit = computed(
    () =>
      !!connections.value.linear && !!description.value.trim() && !busy.value,
  );

  const stopDraftWatch = watch(
    [description, includeSession],
    () => {
      draft.attempt = undefined;
      errors.send = '';
    },
    { flush: 'sync' },
  );
  const stopTeamWatch = watch(
    teamId,
    () => {
      projectId.value = NO_PROJECT;
    },
    { flush: 'sync' },
  );

  async function run<T>(
    operation: Operation,
    action: () => Promise<T>,
  ): Promise<Outcome<T> | undefined> {
    if (disposed || busy.value) return;
    const current = ++generation;
    busy.value = operation;
    errors[operation] = '';
    spinnerTimer = setTimeout(() => {
      if (generation === current) showSpinner.value = true;
    }, 180);
    try {
      const value = await Promise.race([
        action(),
        new Promise<never>((_, reject) => {
          requestTimer = setTimeout(
            () => reject({ code: 'network' }),
            REQUEST_TIMEOUT_MS,
          );
        }),
      ]);
      if (disposed || current !== generation) return;
      return { ok: true, value };
    } catch (error) {
      if (disposed || current !== generation) return;
      const code = cloudErrorCode(error);
      errors[operation] = errorCopy(operation, code);
      return { ok: false, code };
    } finally {
      if (current === generation) {
        clearTimeout(spinnerTimer);
        clearTimeout(requestTimer);
        busy.value = null;
        showSpinner.value = false;
      }
    }
  }

  async function load(): Promise<void> {
    if (busy.value) return;
    view.value = 'loading';
    const result = await run('load', api.readConnections);
    if (!result) return;
    if (!result.ok) {
      view.value = 'load-error';
      return;
    }
    connections.value = result.value;
    loaded = true;
    view.value = result.value.linear ? 'report' : 'linear';
  }

  function openReporter(): void {
    if (disposed) return;
    open.value = true;
    if (!loaded) void load();
  }

  function setOpen(next: boolean): void {
    if (next) openReporter();
    else if (!busy.value) open.value = false;
  }

  function showConnections(): void {
    if (!busy.value) view.value = 'connections';
  }

  function showReport(): void {
    if (!busy.value && connections.value.linear) {
      linearKey.value = '';
      token.value = '';
      view.value = 'report';
    }
  }

  function backToKey(): void {
    if (busy.value) return;
    validated.value = false;
    view.value = 'linear';
  }

  async function validateLinear(): Promise<void> {
    if (!canValidate.value) return;
    const result = await run('linear', () =>
      api.validateLinear(linearKey.value.trim()),
    );
    if (!result?.ok) return;
    if (!result.value.teams.length) {
      errors.linear = 'No teams are available for this key.';
      return;
    }
    teams.value = result.value.teams;
    const previousTeam = teams.value.find(
      (team) => team.id === previousDestination?.teamId,
    );
    const team = previousTeam ?? teams.value[0];
    if (!team) return;
    teamId.value = team.id;
    const previousProject = previousDestination?.projectId;
    projectId.value =
      previousProject &&
      team.projects.some((project) => project.id === previousProject)
        ? previousProject
        : NO_PROJECT;
    validated.value = true;
    if (
      reconnecting.value &&
      previousTeam &&
      (!previousProject || projectId.value === previousProject)
    ) {
      await saveLinear();
    } else if (view.value !== 'connections') view.value = 'destination';
  }

  async function saveLinear(): Promise<void> {
    if (!canSaveDestination.value) return;
    const result = await run('destination', () =>
      api.saveLinear({
        apiKey: linearKey.value.trim(),
        teamId: teamId.value,
        ...(projectId.value !== NO_PROJECT
          ? { projectId: projectId.value }
          : {}),
      }),
    );
    if (!result?.ok) return;
    connections.value = result.value;
    linearKey.value = '';
    validated.value = false;
    if (reconnecting.value) {
      reconnecting.value = false;
      previousDestination = null;
      errors.send = '';
      view.value = 'report';
    } else if (view.value !== 'connections')
      view.value = connections.value.grafana ? 'report' : 'grafana';
  }

  async function saveGrafana(): Promise<void> {
    if (!canSaveGrafana.value) return;
    const result = await run('grafana', () =>
      api.saveGrafana({
        endpoint: endpoint.value.trim(),
        instanceId: instanceId.value.trim(),
        token: token.value.trim(),
      }),
    );
    if (!result?.ok) return;
    connections.value = result.value;
    token.value = '';
  }

  async function disconnect(provider: CloudProvider): Promise<void> {
    const result = await run(`disconnect-${provider}`, () =>
      api.disconnect(provider),
    );
    if (!result?.ok) return;
    connections.value = result.value;
    if (provider === 'linear') {
      linearKey.value = '';
      validated.value = false;
      teamId.value = '';
      projectId.value = NO_PROJECT;
      teams.value = [];
      previousDestination = null;
      reconnecting.value = false;
      errors.linear = '';
      errors.destination = '';
    } else {
      endpoint.value = '';
      instanceId.value = '';
      token.value = '';
      errors.grafana = '';
    }
  }

  async function submitReport(): Promise<void> {
    if (!canSubmit.value) return;
    if (!draft.attempt) {
      const selectedSession = includeSession.value ? sessionId() : undefined;
      draft.attempt = {
        description: description.value.trim(),
        ...(selectedSession ? { sessionId: selectedSession } : {}),
        reportId: crypto.randomUUID(),
        timeUnixNano: `${BigInt(Date.now()) * 1_000_000n}`,
      };
    }
    const attempt = { ...draft.attempt };
    const result = await run('send', () => api.submitReport(attempt));
    if (!result) return;
    if (result.ok) {
      if (draft.attempt?.reportId !== attempt.reportId) return;
      description.value = '';
      includeSession.value = false;
      draft.attempt = undefined;
      open.value = false;
    } else if (
      result.code === 'authentication' ||
      result.code === 'not_configured'
    ) {
      previousDestination = connections.value.linear;
      connections.value = { ...connections.value, linear: null };
      linearKey.value = '';
      validated.value = false;
      reconnecting.value = true;
      errors.send = '';
      view.value = 'linear';
    }
  }

  function submit(): void {
    if (view.value === 'report') void submitReport();
    else if (
      (view.value === 'linear' ||
        view.value === 'destination' ||
        view.value === 'connections') &&
      !connections.value.linear
    ) {
      if (validated.value) void saveLinear();
      else void validateLinear();
    } else if (view.value === 'grafana' && !connections.value.grafana)
      void saveGrafana();
    else showReport();
  }

  function dispose(): void {
    disposed = true;
    generation++;
    clearTimeout(spinnerTimer);
    clearTimeout(requestTimer);
    linearKey.value = '';
    token.value = '';
    busy.value = null;
    stopDraftWatch();
    stopTeamWatch();
  }

  return {
    open,
    view,
    connections,
    busy,
    showSpinner,
    errors,
    description,
    includeSession,
    linearKey,
    teams,
    teamId,
    projectId,
    validated,
    endpoint,
    instanceId,
    token,
    reconnecting,
    teamOptions,
    projectOptions,
    canValidate,
    canSaveDestination,
    canSaveGrafana,
    canSubmit,
    openReporter,
    setOpen,
    load,
    showConnections,
    showReport,
    backToKey,
    validateLinear,
    saveLinear,
    saveGrafana,
    disconnect,
    submitReport,
    submit,
    dispose,
  };
}

export type { IssueDraft };
export { createIssueDraft, createIssueReporter };

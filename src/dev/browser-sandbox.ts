import type { InvokeArgs } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';

import type { SessionSummary, WorkspaceSnapshot } from '../composables/state';
import type { PiBridgeEvent } from '../lib/pi/bridge';
import {
  setSidebarWidthStorage,
  type SidebarWidthStorage,
} from '../lib/sidebar-width';

import workspaceSeed from './workspace-seed.json';

type PiMessage =
  | { role: 'user'; content: string }
  | { role: 'assistant'; content: { type: 'text'; text: string }[] };

interface SessionData {
  id: string;
  path: string;
  name: string;
  modelId: string;
  effort: string;
  messages: PiMessage[];
}

interface RuntimeSession {
  runtimeId: string;
  generation: number;
  projectPath: string;
  streaming: boolean;
  promptGeneration: number;
  session: SessionData;
}

type EmitPiEvent = (event: PiBridgeEvent) => Promise<void>;
type BrowserSandboxHandler = (
  command: string,
  args?: InvokeArgs,
) => Promise<unknown>;

interface BrowserSandboxOptions {
  updateAvailable?: boolean;
  emitAppEvent?: (event: string, payload: unknown) => Promise<void>;
}

type SandboxUpdateStatus =
  'available' | 'prepared' | 'authorized' | 'installing';

interface SandboxQuitRequest {
  requestId: number;
  intent: 'updateRestart';
  operationId: number;
}

const ROOT = '/browser-dev/projects';
const FIXED_NOW = '2026-06-15T10:30:00.000Z';
const MODELS = [
  {
    provider: 'sandbox',
    id: 'tau-dev',
    name: 'Tau Dev',
    reasoning: true,
  },
];

type SeedSession = (typeof workspaceSeed)[number]['sessions'][number];

const seedSessionCount = workspaceSeed.reduce(
  (count, project) => count + project.sessions.length,
  0,
);
const seededMessages = new Map<string, PiMessage[]>(
  workspaceSeed.flatMap(({ sessions }) =>
    sessions.map(({ id, user, assistant }): [string, PiMessage[]] => [
      id,
      [
        { role: 'user', content: user },
        {
          role: 'assistant',
          content: [{ type: 'text', text: assistant }],
        },
      ],
    ]),
  ),
);

function session(
  project: string,
  seed: SeedSession,
  sortAt: number,
  selected = false,
): SessionSummary {
  return {
    id: seed.id,
    path: `${ROOT}/${project}/${seed.id}.jsonl`,
    title: seed.title,
    model: 'tau-dev',
    lastActive: FIXED_NOW,
    lastUserMessageAt: sortAt,
    sortAt,
    archived: false,
    selected,
  };
}

function createBrowserSandboxWorkspace(): WorkspaceSnapshot {
  return {
    activeProjectPath: `${ROOT}/atlas`,
    piPath: '/browser-dev/bin/pi',
    projects: workspaceSeed.map((project, projectIndex) => ({
      path: `${ROOT}/${project.name}`,
      name: project.name,
      workingDirectory: `${ROOT}/${project.name}`,
      collapsed: false,
      selected: projectIndex === 0,
      sessions: project.sessions.map((seed, sessionIndex) =>
        session(
          project.name,
          seed,
          seedSessionCount -
            workspaceSeed
              .slice(0, projectIndex)
              .reduce((count, item) => count + item.sessions.length, 0) -
            sessionIndex,
          projectIndex === 0 && sessionIndex === 0,
        ),
      ),
    })),
  };
}

function record(value: InvokeArgs | undefined): Record<string, unknown> {
  return (value ?? {}) as Record<string, unknown>;
}

function requiredString(args: Record<string, unknown>, key: string): string {
  const value = args[key];
  if (typeof value !== 'string' || !value) {
    throw new Error(`Browser sandbox expected ${key}.`);
  }
  return value;
}

function requiredPositiveInteger(
  args: Record<string, unknown>,
  key: string,
): number {
  const value = args[key];
  if (!Number.isSafeInteger(value) || (value as number) <= 0) {
    throw new Error(`Browser sandbox expected a positive integer ${key}.`);
  }
  return value as number;
}

function requiredBoolean(args: Record<string, unknown>, key: string): boolean {
  const value = args[key];
  if (typeof value !== 'boolean') {
    throw new Error(`Browser sandbox expected ${key}.`);
  }
  return value;
}

function requireOwner(
  args: Record<string, unknown>,
  activeOwner: string,
): void {
  if (!activeOwner || args.ownerId !== activeOwner) {
    throw new Error('Browser sandbox rejected a stale Pi frontend owner.');
  }
}

function createMemorySidebarWidthStorage(): SidebarWidthStorage {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
}

function createBrowserSandboxHandler(
  emitEvent: EmitPiEvent,
  options: BrowserSandboxOptions = {},
): BrowserSandboxHandler {
  const workspace = createBrowserSandboxWorkspace();
  let generation = 1;
  let nextSession = 1;
  let nextPreview = 1;
  let ownershipRevision = 0;
  let activeOwner = '';
  let updateStatus: SandboxUpdateStatus = 'available';
  let pendingQuitRequest: SandboxQuitRequest | null = null;
  let authorizedQuitRequest: SandboxQuitRequest | null = null;
  const updateOperationId = 1;
  const updateVersion = '0.2.0';
  const runtimes = new Map<string, RuntimeSession>();
  const sessions = new Map<string, SessionData>(
    workspace.projects.flatMap((project) =>
      project.sessions.map((summary) => [
        summary.id,
        {
          id: summary.id,
          path: summary.path,
          name: summary.title,
          modelId: 'tau-dev',
          effort: 'medium',
          messages: structuredClone(seededMessages.get(summary.id) ?? []),
        },
      ]),
    ),
  );

  const cloneWorkspace = (): WorkspaceSnapshot => structuredClone(workspace);

  function select(projectPath: string, sessionId = ''): void {
    workspace.activeProjectPath = projectPath;
    for (const project of workspace.projects) {
      project.selected = project.path === projectPath;
      for (const candidate of project.sessions) {
        candidate.selected =
          project.path === projectPath && candidate.id === sessionId;
      }
    }
  }

  function runtimeIsActive(
    runtime: RuntimeSession,
    promptGeneration?: number,
  ): boolean {
    return (
      runtimes.get(runtime.runtimeId) === runtime &&
      (promptGeneration === undefined ||
        runtime.promptGeneration === promptGeneration)
    );
  }

  async function bridge(
    runtime: RuntimeSession,
    value: unknown,
  ): Promise<void> {
    await emitEvent({
      runtimeId: runtime.runtimeId,
      generation: runtime.generation,
      kind: 'rpc',
      line: JSON.stringify(value),
    });
  }

  function response(
    runtime: RuntimeSession,
    request: Record<string, unknown>,
    data?: unknown,
  ): Promise<void> {
    return bridge(runtime, {
      type: 'response',
      id: request.id,
      command: request.type,
      success: true,
      ...(data === undefined ? {} : { data }),
    });
  }

  function stateFor(runtime: RuntimeSession): Record<string, unknown> {
    return {
      sessionId: runtime.session.id,
      sessionFile: runtime.session.path,
      sessionName: runtime.session.name,
      model: {
        provider: 'sandbox',
        id: runtime.session.modelId,
        name: 'Tau Dev',
      },
      thinkingLevel: runtime.session.effort,
      isStreaming: runtime.streaming,
      isCompacting: false,
    };
  }

  async function runPrompt(
    runtime: RuntimeSession,
    request: Record<string, unknown>,
  ): Promise<void> {
    const message = requiredString(request, 'message');
    const promptGeneration = ++runtime.promptGeneration;
    const isActive = (): boolean => runtimeIsActive(runtime, promptGeneration);

    runtime.streaming = true;
    runtime.session.messages.push({ role: 'user', content: message });
    await bridge(runtime, { type: 'agent_start' });
    if (!isActive()) return;
    await bridge(runtime, {
      type: 'message_start',
      message: { role: 'user', content: message },
    });
    if (!isActive()) return;
    await response(runtime, request);
    if (!isActive()) return;

    const reply = `Browser sandbox received: “${message}”\n\nThis reply is generated locally and the workspace will reset on reload.`;
    await bridge(runtime, {
      type: 'message_start',
      message: { role: 'assistant', content: [] },
    });
    if (!isActive()) return;
    await bridge(runtime, {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: reply },
    });
    if (!isActive()) return;
    const assistant: PiMessage = {
      role: 'assistant',
      content: [{ type: 'text', text: reply }],
    };
    runtime.session.messages.push(assistant);
    await bridge(runtime, { type: 'message_end', message: assistant });
    if (!isActive()) return;
    runtime.streaming = false;
    await bridge(runtime, { type: 'agent_settled' });
  }

  async function handlePiRequest(
    runtime: RuntimeSession,
    request: Record<string, unknown>,
  ): Promise<void> {
    switch (request.type) {
      case 'get_available_models':
        await response(runtime, request, { models: MODELS });
        return;
      case 'get_commands':
        await response(runtime, request, { commands: [] });
        return;
      case 'get_state':
        await response(runtime, request, stateFor(runtime));
        return;
      case 'get_available_thinking_levels':
        await response(runtime, request, {
          levels: ['off', 'low', 'medium', 'high'],
        });
        return;
      case 'get_messages':
        await response(runtime, request, {
          messages: structuredClone(runtime.session.messages),
        });
        return;
      case 'get_entries':
        await response(runtime, request, { entries: [], leafId: '' });
        return;
      case 'set_model':
        runtime.session.modelId = requiredString(request, 'modelId');
        await response(runtime, request);
        return;
      case 'set_thinking_level':
        runtime.session.effort = requiredString(request, 'level');
        await response(runtime, request);
        return;
      case 'set_session_name':
        runtime.session.name = requiredString(request, 'name');
        await response(runtime, request);
        if (!runtimeIsActive(runtime)) return;
        await bridge(runtime, {
          type: 'session_info_changed',
          name: runtime.session.name,
        });
        return;
      case 'prompt':
        await runPrompt(runtime, request);
        return;
      case 'abort':
        runtime.promptGeneration += 1;
        runtime.streaming = false;
        await response(runtime, request);
        if (!runtimeIsActive(runtime)) return;
        await bridge(runtime, { type: 'agent_settled' });
        return;
      case 'extension_ui_response':
        return;
      default:
        throw new Error(
          `Unsupported browser sandbox Pi request ${String(request.type)}.`,
        );
    }
  }

  async function handleCommand(
    command: string,
    rawArgs?: InvokeArgs,
  ): Promise<unknown> {
    const args = record(rawArgs);
    if (command === 'read_pi_frontend_revision') return ownershipRevision;
    if (command === 'claim_pi_frontend') {
      const ownerId = requiredString(args, 'ownerId');
      if (activeOwner === ownerId) return null;
      if (args.expectedRevision !== ownershipRevision) {
        throw new Error('Stale Pi frontend ownership revision.');
      }
      ownershipRevision += 1;
      activeOwner = ownerId;
      runtimes.clear();
      return null;
    }
    if (command === 'load_workspace') return cloneWorkspace();
    if (command === 'read_model_scope') return [];
    if (command === 'read_admin_mode') return false;
    if (command === 'get_dismissed_update_version') return null;
    if (command === 'pending_quit_request') return pendingQuitRequest;
    if (command === 'update_snapshot') {
      if (!options.updateAvailable) return { supported: false, status: 'idle' };
      return {
        supported: true,
        status:
          updateStatus === 'available'
            ? 'available'
            : updateStatus === 'installing'
              ? 'installing'
              : 'prepared',
        operationId: updateOperationId,
        candidate: { version: updateVersion },
      };
    }
    if (command === 'check_for_update') {
      return options.updateAvailable
        ? {
            status: 'available',
            version: updateVersion,
            operationId: updateOperationId,
          }
        : { status: 'unavailable' };
    }
    if (command === 'download_update') {
      const operationId = requiredPositiveInteger(args, 'operationId');
      if (
        !options.updateAvailable ||
        operationId !== updateOperationId ||
        updateStatus !== 'available'
      ) {
        throw new Error('Browser sandbox rejected update download arguments.');
      }
      updateStatus = 'prepared';
      return null;
    }
    if (command === 'request_update_restart') {
      const operationId = requiredPositiveInteger(args, 'operationId');
      if (
        !options.updateAvailable ||
        operationId !== updateOperationId ||
        updateStatus !== 'prepared'
      ) {
        throw new Error('Browser sandbox rejected update restart arguments.');
      }
      pendingQuitRequest = {
        requestId: 1,
        intent: 'updateRestart',
        operationId,
      };
      await options.emitAppEvent?.('tau://quit-requested', pendingQuitRequest);
      return null;
    }
    if (command === 'resolve_quit_request') {
      const requestId = requiredPositiveInteger(args, 'requestId');
      const confirmed = requiredBoolean(args, 'confirmed');
      if (!pendingQuitRequest || requestId !== pendingQuitRequest.requestId) {
        return false;
      }
      if (confirmed) {
        authorizedQuitRequest = pendingQuitRequest;
        updateStatus = 'authorized';
      }
      pendingQuitRequest = null;
      return true;
    }
    if (command === 'install_update') {
      const requestId = requiredPositiveInteger(args, 'requestId');
      const operationId = requiredPositiveInteger(args, 'operationId');
      if (
        !options.updateAvailable ||
        updateStatus !== 'authorized' ||
        requestId !== authorizedQuitRequest?.requestId ||
        operationId !== authorizedQuitRequest.operationId
      ) {
        throw new Error('Browser sandbox rejected update install arguments.');
      }
      updateStatus = 'installing';
      authorizedQuitRequest = null;
      return null;
    }
    if (command === 'prepare_file_preview') {
      const path = requiredString(args, 'path');
      if (path.endsWith('/')) return { kind: 'directory' };
      const basePath = requiredString(args, 'basePath');
      const source = `// Browser sandbox preview\nexport const path = ${JSON.stringify(path)};\n`;
      return {
        kind: 'ready',
        id: `browser-preview-${nextPreview++}`,
        assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(source)}`,
        filename: path.split('/').at(-1) ?? 'preview.ts',
        sourcePath: path.startsWith('/') ? path : `${basePath}/${path}`,
        byteLength: source.length,
      };
    }
    if (
      command === 'set_admin_mode' ||
      command === 'set_dismissed_update_version' ||
      command === 'release_file_preview' ||
      command === 'ingest_telemetry' ||
      command === 'submit_issue_report' ||
      command === 'plugin:window|show' ||
      command === 'plugin:opener|open_url' ||
      command === 'plugin:opener|open_path' ||
      command === 'plugin:clipboard-manager|write_text'
    ) {
      return null;
    }
    if (command === 'plugin:dialog|open') {
      return `${ROOT}/playground`;
    }
    if (command === 'start_pi') {
      requireOwner(args, activeOwner);
      const runtimeId = requiredString(args, 'runtimeId');
      const projectPath = requiredString(args, 'projectPath');
      const sessionPath =
        typeof args.sessionPath === 'string' ? args.sessionPath : '';
      const existing = workspace.projects
        .flatMap((project) => project.sessions)
        .find((candidate) => candidate.path === sessionPath);
      const id = existing?.id ?? `browser-session-${nextSession++}`;
      let sessionData = sessions.get(id);
      if (!sessionData) {
        sessionData = {
          id,
          path: existing?.path ?? `${projectPath}/${id}.jsonl`,
          name: existing?.title ?? 'New Session',
          modelId: 'tau-dev',
          effort: 'medium',
          messages: [],
        };
        sessions.set(id, sessionData);
      }
      const runtime: RuntimeSession = {
        runtimeId,
        generation: generation++,
        projectPath,
        streaming: false,
        promptGeneration: 0,
        session: sessionData,
      };
      const replaced = runtimes.get(runtimeId);
      if (replaced) replaced.promptGeneration += 1;
      runtimes.set(runtimeId, runtime);
      await emitEvent({
        runtimeId,
        generation: runtime.generation,
        kind: 'started',
      });
      return runtime.generation;
    }
    if (command === 'send_pi') {
      requireOwner(args, activeOwner);
      const runtime = runtimes.get(requiredString(args, 'runtimeId'));
      if (!runtime) throw new Error('Browser sandbox runtime is not running.');
      const request = args.request;
      if (!request || typeof request !== 'object' || Array.isArray(request)) {
        throw new Error('Browser sandbox expected a Pi request.');
      }
      await handlePiRequest(runtime, request as Record<string, unknown>);
      return null;
    }
    if (command === 'stop_pi') {
      requireOwner(args, activeOwner);
      const runtimeId = requiredString(args, 'runtimeId');
      const runtime = runtimes.get(runtimeId);
      if (runtime) {
        runtime.promptGeneration += 1;
        runtime.streaming = false;
        runtimes.delete(runtimeId);
      }
      return null;
    }
    if (command === 'set_active_project') {
      select(requiredString(args, 'path'));
      return cloneWorkspace();
    }
    if (command === 'set_active_session') {
      select(
        requiredString(args, 'projectPath'),
        requiredString(args, 'sessionId'),
      );
      return cloneWorkspace();
    }
    if (command === 'set_project_collapsed') {
      const project = workspace.projects.find(
        (candidate) => candidate.path === requiredString(args, 'path'),
      );
      if (project) project.collapsed = args.collapsed === true;
      return cloneWorkspace();
    }
    if (command === 'reorder_projects') {
      const paths = Array.isArray(args.projectPaths) ? args.projectPaths : [];
      workspace.projects.sort(
        (a, b) => paths.indexOf(a.path) - paths.indexOf(b.path),
      );
      return cloneWorkspace();
    }
    if (command === 'import_project') {
      const path = requiredString(args, 'path');
      if (!workspace.projects.some((project) => project.path === path)) {
        workspace.projects.push({
          path,
          name: path.split('/').at(-1) || 'project',
          workingDirectory: path,
          collapsed: false,
          selected: false,
          sessions: [],
        });
      }
      return cloneWorkspace();
    }
    if (command === 'remove_project') {
      const path = requiredString(args, 'path');
      workspace.projects = workspace.projects.filter(
        (project) => project.path !== path,
      );
      if (workspace.activeProjectPath === path) {
        const next = workspace.projects[0];
        if (next) select(next.path, next.sessions[0]?.id);
        else workspace.activeProjectPath = '';
      }
      return cloneWorkspace();
    }
    if (command === 'register_session') {
      const projectPath = requiredString(args, 'projectPath');
      const project = workspace.projects.find(
        (candidate) => candidate.path === projectPath,
      );
      if (project) {
        const id = requiredString(args, 'sessionId');
        const existing = project.sessions.find(
          (candidate) => candidate.id === id,
        );
        if (!existing) {
          project.sessions.push({
            id,
            path: requiredString(args, 'sessionPath'),
            title: requiredString(args, 'sessionName'),
            model: 'tau-dev',
            lastActive: 'now',
            lastUserMessageAt:
              typeof args.lastUserMessageAt === 'number'
                ? args.lastUserMessageAt
                : Date.now(),
            sortAt: Date.now(),
            archived: false,
            selected: false,
          });
        } else {
          existing.title = requiredString(args, 'sessionName');
          existing.archived = false;
        }
      }
      return cloneWorkspace();
    }
    if (command === 'archive_session' || command === 'unarchive_session') {
      const sessionId = requiredString(args, 'sessionId');
      const candidate = workspace.projects
        .flatMap((project) => project.sessions)
        .find((item) => item.id === sessionId);
      if (candidate) candidate.archived = command === 'archive_session';
      return cloneWorkspace();
    }
    throw new Error(
      `Unsupported browser sandbox command ${JSON.stringify(command)}.`,
    );
  }

  return handleCommand;
}

function installBrowserSandbox(): void {
  setSidebarWidthStorage(createMemorySidebarWidthStorage());
  const updateAvailable =
    new URLSearchParams(window.location.search).get('test-update') ===
    'available';
  const handleCommand = createBrowserSandboxHandler(
    (event) => emit<PiBridgeEvent>('pi-event', event),
    {
      updateAvailable,
      emitAppEvent: (event, payload) => emit(event, payload),
    },
  );
  mockWindows('main');
  mockIPC(handleCommand, { shouldMockEvents: true });
}

export { createBrowserSandboxHandler, createBrowserSandboxWorkspace };
export default installBrowserSandbox;

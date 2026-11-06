#!/usr/bin/env bun

import assert from 'node:assert/strict';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import { fileURLToPath } from 'node:url';

const STARTUP_TIMEOUT_MS = 10_000;
const REQUEST_TIMEOUT_MS = 10_000;
const EXIT_TIMEOUT_MS = 3_000;
const CLEANUP_TIMEOUT_MS = 1_000;
const MAX_STDOUT_RECORD_CHARS = 1_000_000;
const MAX_VERSION_CHARS = 128;
const HELPER = 'tau-internal-reload-v1';
type JsonRecord = Record<string, unknown>;

class ContractError extends Error {}

class StderrSummary {
  private bytes = 0;
  private lines = 0;
  private sawContent = false;

  add(chunk: Buffer): void {
    this.bytes += chunk.byteLength;
    const text = chunk.toString('utf8');
    this.lines += text.match(/\n/g)?.length ?? 0;
    this.sawContent ||= text.length > 0;
  }

  describe(): string {
    if (!this.sawContent) return 'no stderr output';
    return `${this.bytes} stderr byte(s) across approximately ${Math.max(1, this.lines)} line(s); content withheld`;
  }
}

interface PendingRequest {
  method: string;
  resolve: (response: JsonRecord) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

class PiRpcProcess {
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly stderr = new StderrSummary();
  private readonly pending = new Map<string, PendingRequest>();
  private readonly completionPromise: Promise<{
    code: number | null;
    signal: NodeJS.Signals | null;
  }>;
  readonly notifications: unknown[] = [];
  private stdoutBuffer = '';
  private fatalError: Error | undefined;
  private exited = false;
  private closed = false;

  constructor(
    command: string,
    args: string[],
    cwd: string,
    env: NodeJS.ProcessEnv,
  ) {
    this.child = spawn(command, args, {
      cwd,
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    this.child.stdin.on('error', () =>
      this.fail(new ContractError('Pi input stream failed.')),
    );
    this.child.stderr.on('data', (chunk: Buffer) => this.stderr.add(chunk));

    const decoder = new StringDecoder('utf8');
    this.child.stdout.on('data', (chunk: Buffer) => {
      this.stdoutBuffer += decoder.write(chunk);
      this.consumeStdoutLines();
    });
    this.child.stdout.on('end', () => {
      this.stdoutBuffer += decoder.end();
      if (this.stdoutBuffer.length > 0) {
        this.fail(
          new ContractError(
            'Pi ended stdout with an unterminated JSONL record.',
          ),
        );
      }
    });

    this.completionPromise = new Promise((resolve, reject) => {
      this.child.once('error', () => {
        this.exited = true;
        const contractError = new ContractError(
          `Unable to start Pi. Install pi on PATH or set TAU_PI_PATH.`,
        );
        this.fail(contractError);
        reject(contractError);
      });
      this.child.once('exit', () => {
        this.exited = true;
        if (this.pending.size > 0) {
          this.fail(
            new ContractError(
              `Pi exited before completing the contract (${this.stderr.describe()}).`,
            ),
          );
        }
      });
      this.child.once('close', (code, signal) => {
        this.closed = true;
        resolve({ code, signal });
      });
    });
    void this.completionPromise.catch(() => undefined);
  }

  async request(method: string, fields: JsonRecord = {}): Promise<JsonRecord> {
    if (this.fatalError) throw this.fatalError;
    if (this.exited) throw new ContractError('Pi exited before it was usable.');

    const id = `tau-contract-${randomUUID()}`;
    const response = new Promise<JsonRecord>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(
          new ContractError(
            `Timed out waiting for Pi response to ${method} (${this.stderr.describe()}).`,
          ),
        );
      }, REQUEST_TIMEOUT_MS);
      this.pending.set(id, { method, resolve, reject, timer });
    });

    await new Promise<void>((resolve, reject) => {
      this.child.stdin.write(
        `${JSON.stringify({ ...fields, id, type: method })}\n`,
        (error) => (error ? reject(error) : resolve()),
      );
    }).catch(() => {
      const pending = this.pending.get(id);
      if (pending) clearTimeout(pending.timer);
      this.pending.delete(id);
      throw new ContractError(`Could not write ${method} to Pi.`);
    });

    return response;
  }

  async close(): Promise<void> {
    this.child.stdin.end();
    let result: { code: number | null; signal: NodeJS.Signals | null };
    try {
      result = await withTimeout(
        this.completionPromise,
        EXIT_TIMEOUT_MS,
        'Pi did not exit after its RPC input was closed.',
      );
    } catch (error) {
      throw this.fatalError ?? error;
    }
    if (this.fatalError) throw this.fatalError;
    if (result.code !== 0) {
      throw new ContractError(
        `Pi exited unsuccessfully (code ${String(result.code)}, signal ${String(result.signal)}; ${this.stderr.describe()}).`,
      );
    }
  }

  async cleanup(): Promise<void> {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(new ContractError('Pi contract process was stopped.'));
    }
    this.pending.clear();
    this.child.stdin.destroy();
    if (this.closed) return;

    if (!this.exited) this.child.kill('SIGTERM');
    try {
      await withTimeout(
        this.completionPromise,
        CLEANUP_TIMEOUT_MS,
        'Pi ignored SIGTERM during cleanup.',
      );
    } catch {
      if (!this.exited) this.child.kill('SIGKILL');
      await withTimeout(
        this.completionPromise.catch(() => ({ code: null, signal: null })),
        CLEANUP_TIMEOUT_MS,
        'Pi ignored SIGKILL during cleanup.',
      ).catch(() => undefined);
    }
  }

  private consumeStdoutLines(): void {
    for (
      let newline = this.stdoutBuffer.indexOf('\n');
      newline >= 0;
      newline = this.stdoutBuffer.indexOf('\n')
    ) {
      if (newline > MAX_STDOUT_RECORD_CHARS) {
        this.fail(new ContractError('Pi emitted an oversized stdout record.'));
        return;
      }
      let line = this.stdoutBuffer.slice(0, newline);
      this.stdoutBuffer = this.stdoutBuffer.slice(newline + 1);
      if (line.endsWith('\r')) line = line.slice(0, -1);
      if (!line) {
        this.fail(new ContractError('Pi emitted an empty stdout record.'));
        return;
      }
      this.consumeStdoutLine(line);
      if (this.fatalError) return;
    }
    if (this.stdoutBuffer.length > MAX_STDOUT_RECORD_CHARS) {
      this.fail(new ContractError('Pi emitted an oversized stdout record.'));
    }
  }

  private consumeStdoutLine(line: string): void {
    let value: unknown;
    try {
      value = JSON.parse(line);
    } catch {
      this.fail(
        new ContractError('Pi emitted a malformed JSONL stdout record.'),
      );
      return;
    }

    const record = asRecord(value);
    if (!record) {
      this.fail(
        new ContractError('Pi emitted a non-object JSONL stdout record.'),
      );
      return;
    }
    if (record.type !== 'response') {
      if (
        record.type === 'extension_ui_request' &&
        record.method === 'notify'
      ) {
        this.notifications.push(record.message);
        return;
      }
      this.fail(
        new ContractError(
          'Unexpected Pi event (extension error, dialog, or agent/tool activity).',
        ),
      );
      return;
    }

    const id = typeof record.id === 'string' ? record.id : '';
    const pending = this.pending.get(id);
    if (!pending) {
      this.fail(
        new ContractError(
          'Pi emitted a response with an unknown or duplicate request id.',
        ),
      );
      return;
    }

    clearTimeout(pending.timer);
    this.pending.delete(id);
    if (record.command !== pending.method) {
      pending.reject(
        new ContractError(
          `Pi correlated ${pending.method} to a response for a different command.`,
        ),
      );
      return;
    }
    pending.resolve(record);
  }

  private fail(error: Error): void {
    if (this.fatalError) return;
    this.fatalError = error;
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
  }
}

function asRecord(value: unknown): JsonRecord | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as JsonRecord)
    : undefined;
}

function isolatedEnvironment(root: string): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {
    HOME: root,
    USERPROFILE: root,
    PI_CODING_AGENT_DIR: join(root, 'config'),
    PI_CODING_AGENT_SESSION_DIR: join(root, 'sessions'),
    PI_OFFLINE: '1',
    PI_TELEMETRY: '0',
  };
  for (const name of ['PATH', 'PATHEXT', 'SYSTEMROOT', 'WINDIR', 'COMSPEC']) {
    if (process.env[name] !== undefined) env[name] = process.env[name];
  }
  return env;
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(() => reject(new ContractError(message)), timeoutMs);
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

async function readPiVersion(
  command: string,
  cwd: string,
  env: NodeJS.ProcessEnv,
): Promise<string> {
  const child = spawn(command, ['--version'], {
    cwd,
    env,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const stderr = new StderrSummary();
  let stdout = '';
  child.stdout.on('data', (chunk: Buffer) => {
    stdout += chunk.toString('utf8');
    if (stdout.length > MAX_VERSION_CHARS) child.kill('SIGTERM');
  });
  child.stderr.on('data', (chunk: Buffer) => stderr.add(chunk));

  const completion = new Promise<{ code: number | null }>((resolve, reject) => {
    child.once('error', () =>
      reject(
        new ContractError(
          `Unable to run Pi. Install pi on PATH or set TAU_PI_PATH.`,
        ),
      ),
    );
    child.once('exit', (code) => resolve({ code }));
  });
  void completion.catch(() => undefined);

  let result: { code: number | null };
  try {
    result = await withTimeout(
      completion,
      STARTUP_TIMEOUT_MS,
      'Timed out while checking the Pi version.',
    );
  } catch (error) {
    child.kill('SIGKILL');
    await withTimeout(
      completion.catch(() => ({ code: null })),
      CLEANUP_TIMEOUT_MS,
      'Pi version process did not stop during cleanup.',
    ).catch(() => undefined);
    throw error;
  }

  const version = stdout.trim();
  if (
    result.code !== 0 ||
    !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)
  ) {
    throw new ContractError(
      `Pi did not report a compatible version (${stderr.describe()}).`,
    );
  }
  return version;
}

function data(response: JsonRecord): JsonRecord {
  assert.equal(response.success, true, 'Pi rejected a controlled RPC request.');
  const result = asRecord(response.data);
  assert.ok(result, 'Pi response omitted its data object.');
  return result;
}

async function commands(rpc: PiRpcProcess): Promise<JsonRecord[]> {
  const entries = data(await rpc.request('get_commands')).commands;
  assert.ok(Array.isArray(entries), 'Command catalogue must be an array.');
  return entries.map((entry: unknown) => {
    const record = asRecord(entry);
    assert.ok(record, 'Command catalogue entry must be an object.');
    return record;
  });
}

async function invoke(rpc: PiRpcProcess, name: string): Promise<void> {
  // Fail closed: an unknown extension command would otherwise become a model prompt.
  const found = (await commands(rpc)).filter((entry) => entry.name === name);
  assert.equal(
    found.length,
    1,
    'Controlled extension command must be unambiguous.',
  );
  assert.equal(found[0]?.source, 'extension');
  assert.equal(
    data(await rpc.request('prompt', { message: `/${name}` })).disposition,
    'handled',
    'Controlled command must be handled without starting an agent run.',
  );
}

async function snapshot(rpc: PiRpcProcess): Promise<JsonRecord> {
  const state = data(await rpc.request('get_state'));
  assert.equal(state.isStreaming, false);
  assert.ok(!state.isCompacting);
  assert.equal(state.pendingMessageCount, 0);
  return {
    sessionId: state.sessionId,
    sessionFile: state.sessionFile,
    messages: data(await rpc.request('get_messages')).messages,
    entries: data(await rpc.request('get_entries')),
  };
}

async function fixtures(root: string, revision: number): Promise<void> {
  for (const directory of ['extensions', 'skills', 'prompts']) {
    await rm(join(root, directory), { recursive: true, force: true });
    await mkdir(join(root, directory));
  }
  // The explicit file is re-read by Pi's resource loader, not by the probe.
  await writeFile(join(root, 'context.md'), `TAU_CONTEXT_REVISION_${revision}`);
  if (revision === 0) {
    await writeFile(
      join(root, 'extensions', 'fixture.ts'),
      'export default function() {}',
    );
    return;
  }
  await writeFile(
    join(root, 'extensions', 'fixture.ts'),
    `export default function(pi) {
      pi.registerCommand('fixture-revision-${revision}', {
        description: 'fixture extension ${revision}',
        handler: async (_args, ctx) => ctx.ui.notify('fixture executed ${revision}', 'info'),
      });
    }`,
  );
  await mkdir(join(root, 'skills', 'fixture-skill'));
  await writeFile(
    join(root, 'skills', 'fixture-skill', 'SKILL.md'),
    `---\nname: fixture-skill-${revision}\ndescription: TAU_SKILL_REVISION_${revision}\n---\nControlled skill ${revision}.\n`,
  );
  await writeFile(
    join(root, 'prompts', `fixture-prompt-${revision}.md`),
    `---\ndescription: fixture prompt ${revision}\n---\nControlled prompt ${revision}.\n`,
  );
}

async function verifyResources(
  rpc: PiRpcProcess,
  revision: number,
): Promise<void> {
  const catalogue = await commands(rpc);
  const expected = [HELPER, 'fixture-probe'];
  if (revision !== 0) {
    expected.push(
      `fixture-revision-${revision}`,
      `fixture-prompt-${revision}`,
      `skill:fixture-skill-${revision}`,
    );
  }
  assert.deepEqual(
    catalogue.map((entry) => entry.name).sort(),
    expected.sort(),
    'Reload must replace the catalogue, including additions and removals.',
  );
  if (revision !== 0) {
    assert.equal(
      catalogue.find((entry) => entry.name === `fixture-prompt-${revision}`)
        ?.description,
      `fixture prompt ${revision}`,
    );
    assert.equal(
      catalogue.find(
        (entry) => entry.name === `skill:fixture-skill-${revision}`,
      )?.description,
      `TAU_SKILL_REVISION_${revision}`,
    );
    const before = rpc.notifications.length;
    await invoke(rpc, `fixture-revision-${revision}`);
    assert.deepEqual(
      rpc.notifications.splice(before),
      [`fixture executed ${revision}`],
      'Reload must execute the new extension implementation.',
    );
  }
  const before = rpc.notifications.length;
  await invoke(rpc, 'fixture-probe');
  const notifications = rpc.notifications.splice(before);
  assert.equal(
    notifications.length,
    1,
    'Probe must emit exactly one snapshot.',
  );
  assert.equal(typeof notifications[0], 'string');
  const probe = asRecord(JSON.parse(String(notifications[0])) as unknown);
  assert.ok(probe);
  assert.deepEqual(probe.tools, [], 'No tools may be active.');
  assert.deepEqual(
    probe.contextRevisions,
    [String(revision)],
    'Explicit context file changes must take effect.',
  );
}

async function main(): Promise<void> {
  const root = await mkdtemp(join(tmpdir(), 'tau-pi-reload-contract-'));
  let rpc: PiRpcProcess | undefined;
  try {
    const config = join(root, 'config');
    const sessions = join(root, 'sessions');
    await mkdir(config);
    await mkdir(sessions);
    const env = isolatedEnvironment(root);
    const selected = process.env.TAU_PI_PATH?.trim() || 'pi';
    const command =
      selected.includes('/') || selected.includes('\\')
        ? resolve(selected)
        : selected;
    const version = await readPiVersion(command, root, env);
    console.log(`Testing reload contract with pi ${version}.`);
    await fixtures(root, 1);
    const probePath = join(root, 'probe.ts');
    await writeFile(
      probePath,
      `export default function(pi) {
      pi.registerCommand('fixture-probe', {
        handler: async (_args, ctx) => {
          const prompt = ctx.getSystemPrompt();
          ctx.ui.notify(JSON.stringify({
            tools: pi.getActiveTools(),
            contextRevisions: [...prompt.matchAll(/TAU_CONTEXT_REVISION_(\\d+)/g)].map(m => m[1]),
          }), 'info');
        },
      });
    }`,
    );
    const sessionPath = join(sessions, 'fixture.jsonl');
    const timestamp = new Date().toISOString();
    const message = {
      role: 'user',
      content: 'Controlled pre-existing conversation.',
      timestamp: Date.now(),
    };
    await writeFile(
      sessionPath,
      [
        { type: 'session', version: 3, id: randomUUID(), timestamp, cwd: root },
        { type: 'message', id: '1234abcd', parentId: null, timestamp, message },
      ]
        .map((entry) => JSON.stringify(entry))
        .join('\n') + '\n',
    );
    const helper = fileURLToPath(
      new URL('../src-tauri/pi-extensions/tau-reload.ts', import.meta.url),
    );
    // Copy only bundled source into the isolated root; no repository discovery/imports.
    const isolatedHelper = join(root, 'tau-reload.ts');
    await writeFile(isolatedHelper, await readFile(helper));
    rpc = new PiRpcProcess(
      command,
      [
        '--mode',
        'rpc',
        '--offline',
        '--no-tools',
        '--no-mcp',
        '--no-extensions',
        '--no-skills',
        '--no-prompt-templates',
        '--no-context-files',
        '--no-themes',
        '--no-approve',
        '--session-dir',
        sessions,
        '--session',
        sessionPath,
        '--extension',
        isolatedHelper,
        '--extension',
        probePath,
        '--extension',
        join(root, 'extensions', 'fixture.ts'),
        '--skill',
        join(root, 'skills'),
        '--prompt-template',
        join(root, 'prompts'),
        '--append-system-prompt',
        join(root, 'context.md'),
      ],
      root,
      env,
    );
    const original = await snapshot(rpc);
    assert.equal(typeof original.sessionId, 'string');
    assert.equal(original.sessionFile, sessionPath);
    assert.deepEqual(
      original.messages,
      [message],
      'Seeded conversation must be loaded.',
    );
    await verifyResources(rpc, 1);
    for (const revision of [2, 0, 3, 3]) {
      await fixtures(root, revision);
      await invoke(rpc, HELPER);
      await verifyResources(rpc, revision);
      assert.deepEqual(
        await snapshot(rpc),
        original,
        'Reload must preserve session identity, conversation, entries, and leaf.',
      );
    }
    assert.deepEqual(
      rpc.notifications,
      [],
      'Unexpected extension notifications.',
    );
    await rpc.close();
    console.log(
      `Pi reload contract passed with pi ${version}: four reloads; extension execution, skill/prompt catalogue replacement, explicit context-file refresh, helper rediscovery, unchanged identity/conversation; clean EOF shutdown.`,
    );
    console.log(
      'Context discovery is disabled; context coverage uses an explicit --append-system-prompt file. Older supported baseline (0.84.x) not tested by this invocation; run separately with TAU_PI_PATH when available.',
    );
  } finally {
    try {
      await rpc?.cleanup();
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  }
}

try {
  await main();
} catch (error) {
  console.error(
    `Pi reload contract failed: ${error instanceof Error ? error.message : 'unknown incompatibility'}`,
  );
  process.exitCode = 1;
}

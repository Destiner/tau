#!/usr/bin/env bun

import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { StringDecoder } from 'node:string_decoder';

const STARTUP_TIMEOUT_MS = 10_000;
const REQUEST_TIMEOUT_MS = 5_000;
const EXIT_TIMEOUT_MS = 3_000;
const CLEANUP_TIMEOUT_MS = 1_000;
const MAX_STDOUT_RECORD_CHARS = 1_000_000;
const MAX_VERSION_CHARS = 128;

const methods = [
  'get_available_models',
  'get_commands',
  'get_state',
  'get_available_thinking_levels',
  'get_messages',
  'get_entries',
] as const;

type Method = (typeof methods)[number];
type JsonRecord = Record<string, unknown>;

const thinkingLevels = new Set([
  'off',
  'minimal',
  'low',
  'medium',
  'high',
  'xhigh',
  'max',
]);

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
  method: Method;
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
      this.child.once('error', (error) => {
        this.exited = true;
        const contractError = new ContractError(
          `Unable to start Pi. Install pi on PATH or set TAU_PI_PATH (${error.message}).`,
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

  async request(method: Method): Promise<JsonRecord> {
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
        `${JSON.stringify({ id, type: method })}\n`,
        (error) => (error ? reject(error) : resolve()),
      );
    }).catch((error: unknown) => {
      const pending = this.pending.get(id);
      if (pending) clearTimeout(pending.timer);
      this.pending.delete(id);
      throw new ContractError(
        `Could not write ${method} to Pi (${error instanceof Error ? error.message : 'unknown write error'}).`,
      );
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
    if (record.type !== 'response') return;

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

function requireRecord(value: unknown, label: string): JsonRecord {
  const record = asRecord(value);
  if (!record) throw new ContractError(`${label} must be an object.`);
  return record;
}

function requireArray(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value)) {
    throw new ContractError(`${label} must be an array.`);
  }
  return value;
}

function requireNonEmptyString(value: unknown, label: string): string {
  if (typeof value !== 'string' || value.length === 0) {
    throw new ContractError(`${label} must be a non-empty string.`);
  }
  return value;
}

function requireModel(value: unknown, label: string): void {
  const model = requireRecord(value, label);
  requireNonEmptyString(model.provider, `${label}.provider`);
  requireNonEmptyString(model.id, `${label}.id`);
}

function responseData(response: JsonRecord, method: Method): JsonRecord {
  if (response.type !== 'response' || response.command !== method) {
    throw new ContractError(
      `${method} returned an incompatible response envelope.`,
    );
  }
  if (response.success !== true) {
    throw new ContractError(`${method} was rejected by Pi.`);
  }
  return requireRecord(response.data, `${method}.data`);
}

function validateModels(response: JsonRecord): void {
  const models = requireArray(
    responseData(response, 'get_available_models').models,
    'get_available_models.data.models',
  );
  for (const [index, model] of models.entries()) {
    requireModel(model, `get_available_models.data.models[${index}]`);
  }
}

function validateCommands(response: JsonRecord): void {
  const commands = requireArray(
    responseData(response, 'get_commands').commands,
    'get_commands.data.commands',
  );
  for (const [index, command] of commands.entries()) {
    const entry = requireRecord(
      command,
      `get_commands.data.commands[${index}]`,
    );
    requireNonEmptyString(
      entry.name,
      `get_commands.data.commands[${index}].name`,
    );
  }
}

function validateState(response: JsonRecord): string {
  const state = responseData(response, 'get_state');
  if (state.model !== null) requireModel(state.model, 'get_state.data.model');
  const thinkingLevel = requireNonEmptyString(
    state.thinkingLevel,
    'get_state.data.thinkingLevel',
  );
  if (!thinkingLevels.has(thinkingLevel)) {
    throw new ContractError('get_state.data.thinkingLevel is unsupported.');
  }
  if (typeof state.isStreaming !== 'boolean') {
    throw new ContractError('get_state.data.isStreaming must be a boolean.');
  }
  if (state.isStreaming) {
    throw new ContractError('A new isolated Pi RPC session started streaming.');
  }
  requireNonEmptyString(state.sessionId, 'get_state.data.sessionId');
  requireNonEmptyString(state.sessionFile, 'get_state.data.sessionFile');
  return thinkingLevel;
}

function validateThinkingLevels(
  response: JsonRecord,
  currentThinkingLevel: string,
): void {
  const levels = requireArray(
    responseData(response, 'get_available_thinking_levels').levels,
    'get_available_thinking_levels.data.levels',
  );
  if (levels.length === 0) {
    throw new ContractError(
      'get_available_thinking_levels.data.levels must not be empty.',
    );
  }
  for (const level of levels) {
    if (typeof level !== 'string' || !thinkingLevels.has(level)) {
      throw new ContractError(
        'get_available_thinking_levels returned an unsupported level.',
      );
    }
  }
  if (!levels.includes(currentThinkingLevel)) {
    throw new ContractError(
      'get_available_thinking_levels omitted the current thinking level.',
    );
  }
}

function validateMessages(response: JsonRecord): void {
  requireArray(
    responseData(response, 'get_messages').messages,
    'get_messages.data.messages',
  );
}

function validateEntries(response: JsonRecord): void {
  const data = responseData(response, 'get_entries');
  requireArray(data.entries, 'get_entries.data.entries');
  if (data.leafId !== null && typeof data.leafId !== 'string') {
    throw new ContractError(
      'get_entries.data.leafId must be a string or null.',
    );
  }
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
    child.once('error', (error) =>
      reject(
        new ContractError(
          `Unable to run Pi. Install pi on PATH or set TAU_PI_PATH (${error.message}).`,
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

async function main(): Promise<void> {
  const root = await mkdtemp(join(tmpdir(), 'tau-pi-contract-'));
  const configDirectory = join(root, 'config');
  const sessionDirectory = join(root, 'sessions');
  let rpc: PiRpcProcess | undefined;

  try {
    await mkdir(configDirectory, { recursive: true });
    await mkdir(sessionDirectory, { recursive: true });
    const command = process.env.TAU_PI_PATH?.trim() || 'pi';
    const env = isolatedEnvironment(root);
    const version = await readPiVersion(command, root, env);
    rpc = new PiRpcProcess(
      command,
      [
        '--mode',
        'rpc',
        '--offline',
        '--no-tools',
        '--no-extensions',
        '--no-skills',
        '--no-prompt-templates',
        '--no-context-files',
        '--no-themes',
        '--no-approve',
        '--session-dir',
        sessionDirectory,
      ],
      root,
      env,
    );

    validateModels(await rpc.request('get_available_models'));
    validateCommands(await rpc.request('get_commands'));
    const thinkingLevel = validateState(await rpc.request('get_state'));
    validateThinkingLevels(
      await rpc.request('get_available_thinking_levels'),
      thinkingLevel,
    );
    validateMessages(await rpc.request('get_messages'));
    validateEntries(await rpc.request('get_entries'));
    await rpc.close();

    console.log(
      `Pi RPC contract passed with pi ${version}: ${methods.join(', ')}.`,
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
    `Pi RPC contract failed: ${error instanceof Error ? error.message : 'unknown incompatibility'}`,
  );
  process.exitCode = 1;
}

import { createHash } from 'node:crypto';
import {
  closeSync,
  fstatSync,
  lstatSync,
  openSync,
  readFileSync,
  readSync,
  realpathSync,
  statSync,
} from 'node:fs';
import { isIP } from 'node:net';
import { homedir, hostname, userInfo } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { Writable } from 'node:stream';

// Keep the identity capture in sync with the native project-trust helper.
const packages = new Set([
  '@earendil-works/pi-coding-agent',
  '@mariozechner/pi-coding-agent',
]);

function within(root: string, path: string) {
  const distance = relative(root, path);
  return (
    !isAbsolute(distance) &&
    distance !== '..' &&
    !distance.startsWith(`..${sep}`)
  );
}

function canonicalDirectory(path: string) {
  if (typeof path !== 'string' || !isAbsolute(path)) throw 'mismatch';
  try {
    const folder = realpathSync.native(path);
    if (statSync(folder).isDirectory()) return folder;
  } catch {}
  throw 'mismatch';
}

function fileId(path: string) {
  const { dev, ino } = statSync(path, { bigint: true });
  if (ino === 0n) throw 'unsupported';
  return `${dev}:${ino}`;
}

function externalFile(path: string, folder: string) {
  const physical = realpathSync.native(path);
  if (within(folder, physical) || !statSync(physical).isFile())
    throw 'unsupported';
  return physical;
}

function executableShebang(path: string) {
  const fd = openSync(path, 'r');
  try {
    if (!fstatSync(fd).isFile()) throw 'unsupported';
    const buffer = Buffer.alloc(256);
    const length = readSync(fd, buffer, 0, buffer.length, 0);
    return (
      buffer.subarray(0, length).toString().split('\n', 1)[0] ?? ''
    ).trimEnd();
  } finally {
    closeSync(fd);
  }
}

function fingerprint(paths: string[], node: string) {
  const hash = createHash('sha256');
  // A Node executable can be large; bind its inode and change times without reading it.
  const { size, mtimeNs, ctimeNs } = statSync(node, { bigint: true });
  hash.update(
    JSON.stringify([node, String(size), String(mtimeNs), String(ctimeNs)]),
  );
  const buffer = Buffer.alloc(128 * 1024);
  for (const path of paths) {
    const fd = openSync(path, 'r');
    try {
      const before = fstatSync(fd, { bigint: true });
      if (!before.isFile() || before.size > 16n * 1024n * 1024n)
        throw 'unsupported';
      const file = createHash('sha256');
      for (let bytes; (bytes = readSync(fd, buffer, 0, buffer.length, null));) {
        file.update(buffer.subarray(0, bytes));
      }
      const after = fstatSync(fd, { bigint: true });
      if (
        before.size !== after.size ||
        before.mtimeNs !== after.mtimeNs ||
        before.ctimeNs !== after.ctimeNs
      ) {
        throw 'unsupported';
      }
      hash.update(JSON.stringify([path, file.digest('hex')]));
    } finally {
      closeSync(fd);
    }
  }
  return hash.digest('hex');
}

function installationFor(folder: string, cli: string, runtimeNode: string) {
  try {
    if (
      !isAbsolute(cli) ||
      !isAbsolute(runtimeNode) ||
      process.env.NODE_OPTIONS ||
      process.env.PI_PACKAGE_DIR
    ) {
      throw 'unsupported';
    }
    const executable = externalFile(cli, folder);
    const node = externalFile(process.execPath, folder);
    const shebang = executableShebang(executable);
    const interpreter =
      shebang === '#!/usr/bin/env node' ? runtimeNode : shebang.slice(2);
    if (
      !shebang.startsWith('#!') ||
      !isAbsolute(interpreter) ||
      externalFile(interpreter, folder) !== node
    ) {
      throw 'unsupported';
    }
    for (let dir = dirname(executable); ; dir = dirname(dir)) {
      if (within(folder, dir)) throw 'unsupported';
      const candidate = join(dir, 'package.json');
      try {
        lstatSync(candidate);
      } catch (error) {
        if (
          (error as NodeJS.ErrnoException)?.code !== 'ENOENT' ||
          dir === dirname(dir)
        )
          throw error;
        continue;
      }
      const manifest = externalFile(candidate, folder);
      if (!within(dir, manifest)) throw 'unsupported';
      const pkg = JSON.parse(readFileSync(manifest, 'utf8'));
      const bin = pkg.bin?.pi;
      const exported = pkg.exports?.['.']?.import;
      if (
        !packages.has(pkg.name) ||
        typeof pkg.version !== 'string' ||
        !pkg.version ||
        typeof bin !== 'string' ||
        resolve(dir, bin) !== executable ||
        typeof exported !== 'string' ||
        !exported.startsWith('./') ||
        !within(dir, resolve(dir, exported))
      )
        throw 'unsupported';
      const entry = externalFile(resolve(dir, exported), folder);
      if (!within(dir, entry)) throw 'unsupported';
      const files = [manifest, executable, entry];
      for (const name of [
        'dist/config.js',
        'dist/core/trust-manager.js',
        'dist/utils/paths.js',
        'dist/bundle/cli-runtime.js',
      ]) {
        const path = join(dir, name);
        try {
          lstatSync(path);
        } catch (error) {
          if ((error as NodeJS.ErrnoException)?.code !== 'ENOENT') throw error;
          // Check an existing parent even when the final file is absent.
          if (within(folder, physicalPath(path))) throw 'unsupported';
          continue;
        }
        const physical = externalFile(path, folder);
        if (!within(dir, physical)) throw 'unsupported';
        files.push(physical);
      }
      return {
        piConfig: pkg.piConfig,
        installation: {
          executable,
          launcherId: fileId(executable),
          entry,
          sdkId: fileId(entry),
          version: pkg.version,
          node,
          nodeId: fileId(node),
          manifest,
          fingerprint: fingerprint(files, node),
        },
      };
    }
  } catch {
    throw 'unsupported';
  }
}

function accountIdentity() {
  const { username: user, uid } = userInfo();
  const host = hostname();
  if (
    !user ||
    !host ||
    uid < 0 ||
    process.getuid?.() !== uid ||
    process.geteuid?.() !== uid
  )
    throw 'unsupported';
  const connection = process.env.SSH_CONNECTION;
  let ssh = '';
  if (connection !== undefined) {
    const [client, clientPort, server, serverPort, ...extra] = connection
      .trim()
      .split(/\s+/);
    const port = (value: string) =>
      /^\d{1,5}$/.test(value) && Number(value) > 0 && Number(value) <= 65535;
    if (
      !client ||
      !clientPort ||
      !server ||
      !serverPort ||
      extra.length > 0 ||
      !isIP(client) ||
      !port(clientPort) ||
      !isIP(server) ||
      !port(serverPort)
    ) {
      throw 'unsupported';
    }
    ssh = `${client} ${server} ${Number(serverPort)}`;
  } else if (process.env.SSH_CLIENT || process.env.SSH_TTY) {
    throw 'unsupported';
  }
  return { user, uid, host, ssh };
}

// Match Pi's manifest defaults and trust-store path normalization without loading its SDK.
function configuredAgentDir(
  config: { name?: unknown; configDir?: unknown } | undefined,
) {
  if (
    config !== undefined &&
    (!config || typeof config !== 'object' || Array.isArray(config))
  )
    throw 'unsupported';
  const name = config?.name || 'pi';
  const configDir = config?.configDir || '.pi';
  if (
    typeof name !== 'string' ||
    !/^[a-z][a-z0-9_-]*$/i.test(name) ||
    typeof configDir !== 'string' ||
    !/^\.?[a-z0-9][a-z0-9_.-]*$/i.test(configDir)
  )
    throw 'unsupported';
  const home = homedir();
  const override = process.env[`${name.toUpperCase()}_CODING_AGENT_DIR`];
  if (override && isAbsolute(override)) return resolve(override);
  if (!isAbsolute(home)) throw 'unsupported';
  if (override === '~') return resolve(home);
  if (override?.startsWith('~/')) return join(home, override.slice(2));
  if (override) throw 'unsupported';
  return join(home, configDir, 'agent');
}

function physicalPath(path: string): string {
  try {
    return realpathSync.native(path);
  } catch (error) {
    if (
      (error as NodeJS.ErrnoException)?.code !== 'ENOENT' ||
      dirname(path) === path
    )
      throw error;
    if (lstatSync(path, { throwIfNoEntry: false })) throw error;
    return join(physicalPath(dirname(path)), relative(dirname(path), path));
  }
}

function startupIdentity() {
  try {
    const cli = process.env.TAU_PROJECT_TRUST_CLI;
    const launched = process.argv[1];
    if (
      !cli ||
      !isAbsolute(cli) ||
      !launched ||
      !isAbsolute(launched) ||
      realpathSync.native(cli) !== realpathSync.native(launched) ||
      process.execArgv.length
    ) {
      return null;
    }
    const folder = canonicalDirectory(process.cwd());
    const folderId = fileId('.');
    if (fileId(folder) !== folderId) return null;
    const { installation, piConfig } = installationFor(
      folder,
      cli,
      process.execPath,
    );
    const agentDir = canonicalDirectory(configuredAgentDir(piConfig));
    return {
      ...installation,
      folder,
      folderId,
      ...accountIdentity(),
      agentDir,
      agentId: fileId(agentDir),
    };
  } catch {
    return null;
  }
}

type ReloadContext = {
  isIdle?: () => boolean;
  reload?: () => Promise<void>;
};

type TrustContext = {
  cwd?: string;
  isProjectTrusted?: () => boolean;
  sessionManager?: {
    getSessionId?: () => string;
    getSessionFile?: () => string | undefined;
  };
  ui?: { setStatus?: (key: string, text: string) => void };
};

type ReloadAPI = {
  on?: (
    event: 'session_start',
    handler: (event: unknown, ctx: TrustContext) => void,
  ) => void;
  registerCommand: (
    name: string,
    command: {
      description: string;
      handler: (args: string, ctx: ReloadContext) => Promise<void>;
    },
  ) => void;
};

export default function (pi: ReloadAPI): void {
  const nonce = process.env.TAU_PROJECT_TRUST_NONCE;
  const privateNonce =
    nonce && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(nonce)
      ? nonce
      : null;
  const identity = privateNonce ? startupIdentity() : null;
  pi.on?.('session_start', (_event, ctx) => {
    if (
      typeof ctx.isProjectTrusted !== 'function' ||
      typeof ctx.sessionManager?.getSessionId !== 'function' ||
      typeof ctx.sessionManager.getSessionFile !== 'function'
    ) {
      return;
    }
    const trusted = ctx.isProjectTrusted();
    if (typeof trusted !== 'boolean') return;
    const sessionId = ctx.sessionManager.getSessionId();
    const sessionFile = ctx.sessionManager.getSessionFile() ?? null;
    if (
      privateNonce &&
      ctx.cwd &&
      typeof sessionId === 'string' &&
      sessionId &&
      (sessionFile === null || typeof sessionFile === 'string')
    ) {
      // Pi redirects the instance method to stderr in RPC mode.
      Writable.prototype.write.call(
        process.stdout,
        `__TAU_PI_TRUST__${JSON.stringify({ nonce: privateNonce, cwd: ctx.cwd, sessionId, sessionFile, trusted, identity })}\n`,
        'utf8',
      );
    }
    if (ctx.cwd && typeof ctx.ui?.setStatus === 'function') {
      ctx.ui.setStatus(
        'tau-internal-project-trust-v1',
        JSON.stringify({
          version: 1,
          trusted,
          cwd: ctx.cwd,
          sessionId,
          sessionFile,
        }),
      );
    }
  });

  pi.registerCommand('tau-internal-reload-v1', {
    description: 'Private Tau resource reload bridge',
    handler: async (args, ctx) => {
      if (args.trim()) {
        throw new Error('Tau reload does not accept arguments.');
      }
      if (
        typeof ctx.isIdle !== 'function' ||
        typeof ctx.reload !== 'function'
      ) {
        throw new Error(
          'This Pi version does not support Tau resource reload.',
        );
      }
      if (!ctx.isIdle()) {
        throw new Error('Pi is busy. Wait for the current work to finish.');
      }
      // Reload invalidates this runtime. Never access its context afterward.
      try {
        await ctx.reload();
      } catch {
        throw new Error(
          'Pi resource reload failed. Review extension errors and try again.',
        );
      }
    },
  });
}

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
import * as module from 'node:module';
import { isIP } from 'node:net';
import { homedir, hostname, userInfo } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const [pi, input, runtimeNode = process.execPath] = process.argv.slice(2);
const failures = new Set([
  'mismatch',
  'unsupported',
  'unavailable',
  'unconfirmed',
]);
let request;
let dispatched = false;

// Keep the identity capture in sync with the standalone tau-reload extension.
const packages = new Set([
  '@earendil-works/pi-coding-agent',
  '@mariozechner/pi-coding-agent',
]);

function within(root, path) {
  const distance = relative(root, path);
  return (
    !isAbsolute(distance) &&
    distance !== '..' &&
    !distance.startsWith(`..${sep}`)
  );
}

function canonicalDirectory(path) {
  if (typeof path !== 'string' || !isAbsolute(path)) throw 'mismatch';
  try {
    const folder = realpathSync.native(path);
    if (statSync(folder).isDirectory()) return folder;
  } catch {}
  throw 'mismatch';
}

function fileId(path) {
  const { dev, ino } = statSync(path, { bigint: true });
  if (ino === 0n) throw 'unsupported';
  return `${dev}:${ino}`;
}

function externalFile(path, folder) {
  const physical = realpathSync.native(path);
  if (within(folder, physical) || !statSync(physical).isFile())
    throw 'unsupported';
  return physical;
}

function executableShebang(path) {
  const fd = openSync(path, 'r');
  try {
    if (!fstatSync(fd).isFile()) throw 'unsupported';
    const buffer = Buffer.alloc(256);
    const length = readSync(fd, buffer, 0, buffer.length, 0);
    return buffer.subarray(0, length).toString().split('\n', 1)[0].trimEnd();
  } finally {
    closeSync(fd);
  }
}

function fingerprint(paths, node) {
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

function installationFor(folder, cli, runtimeNode) {
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
        if (error?.code !== 'ENOENT' || dir === dirname(dir)) throw error;
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
          if (error?.code !== 'ENOENT') throw error;
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
    const parts = connection.trim().split(/\s+/);
    const port = (value) =>
      /^\d{1,5}$/.test(value) && Number(value) > 0 && Number(value) <= 65535;
    if (
      parts.length !== 4 ||
      !isIP(parts[0]) ||
      !port(parts[1]) ||
      !isIP(parts[2]) ||
      !port(parts[3])
    ) {
      throw 'unsupported';
    }
    ssh = `${parts[0]} ${parts[2]} ${Number(parts[3])}`;
  } else if (process.env.SSH_CLIENT || process.env.SSH_TTY) {
    throw 'unsupported';
  }
  return { user, uid, host, ssh };
}

// Match Pi's manifest defaults and trust-store path normalization without loading its SDK.
function configuredAgentDir(config) {
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

// Legacy canaries can use a disposable agent directory that Pi has not created.
function physicalPath(path) {
  try {
    return realpathSync.native(path);
  } catch (error) {
    if (error?.code !== 'ENOENT' || dirname(path) === path) throw error;
    if (lstatSync(path, { throwIfNoEntry: false })) throw error;
    return join(physicalPath(dirname(path)), relative(dirname(path), path));
  }
}

function checkStore(agentDir) {
  const metadata = lstatSync(join(agentDir, 'trust.json'), {
    throwIfNoEntry: false,
  });
  if (metadata && (!metadata.isFile() || metadata.nlink !== 1))
    throw 'unavailable';
}

function folderForRequest() {
  const folder = canonicalDirectory(request.projectFolder);
  if (canonicalDirectory(request.runtimeCwd) !== folder) throw 'mismatch';
  return folder;
}

function record(value) {
  return value && typeof value === 'object' && !Array.isArray(value);
}

function containsIdentity(before, after) {
  return (
    record(before) &&
    Object.keys(after).every(
      (key) => Object.hasOwn(before, key) && before[key] === after[key],
    )
  );
}

function sameIdentity(before, after) {
  return (
    containsIdentity(before, after) &&
    Object.keys(before).length === Object.keys(after).length
  );
}

function agentIdentity(path, native) {
  if (native) {
    const agentDir = canonicalDirectory(path);
    return { agentDir, agentId: fileId(agentDir) };
  }
  const agentDir = physicalPath(path);
  const info = statSync(agentDir, { throwIfNoEntry: false });
  if (info && !info.isDirectory()) throw 'unavailable';
  return { agentDir, agentId: info ? fileId(agentDir) : null };
}

async function execute() {
  if (request.operation !== 'inspect' && request.operation !== 'grant')
    throw 'unavailable';
  const grant = request.operation === 'grant';
  const native = Object.hasOwn(request, 'startup');
  const { startup, previous } = request;
  if (native && !record(startup)) throw 'unsupported';
  if ((grant || previous != null) && !record(previous)) throw 'mismatch';
  const expected = [startup, previous].filter(record);
  const baseIdentity = () => {
    const folder = folderForRequest();
    const { installation, piConfig } = installationFor(folder, pi, runtimeNode);
    return {
      identity: {
        ...installation,
        folder,
        folderId: fileId(folder),
        ...accountIdentity(),
      },
      piConfig,
    };
  };
  const { identity: base, piConfig } = baseIdentity();
  for (const prior of expected) {
    if (!containsIdentity(prior, base)) throw 'mismatch';
  }
  const startupAgent = native
    ? agentIdentity(configuredAgentDir(piConfig), true)
    : null;
  if (
    startupAgent &&
    expected.some((prior) => !sameIdentity(prior, { ...base, ...startupAgent }))
  )
    throw 'mismatch';

  let sdk;
  try {
    // Guard transitive imports too; this hook exists only in the helper process.
    module.registerHooks({
      load(url, context, nextLoad) {
        if (!url.startsWith('node:'))
          externalFile(fileURLToPath(url), base.folder);
        return nextLoad(url, context);
      },
    });
    sdk = await import(pathToFileURL(base.entry).href);
  } catch {
    throw 'unsupported';
  }
  const { getAgentDir, ProjectTrustStore } = sdk;
  if (
    typeof getAgentDir !== 'function' ||
    typeof ProjectTrustStore?.prototype?.getEntry !== 'function' ||
    typeof ProjectTrustStore?.prototype?.set !== 'function'
  )
    throw 'unsupported';

  const verify = (prior, allowCreation = false) => {
    const current = baseIdentity().identity;
    if (!sameIdentity(base, current)) throw 'mismatch';
    const identity = {
      ...current,
      ...agentIdentity(resolve(base.folder, getAgentDir()), native),
    };
    if (startupAgent && !sameIdentity({ ...base, ...startupAgent }, identity))
      throw 'mismatch';
    for (const before of prior) {
      const comparison =
        allowCreation && !native && before.agentId === null
          ? { ...identity, agentId: null }
          : identity;
      if (!sameIdentity(before, comparison)) throw 'mismatch';
    }
    checkStore(identity.agentDir);
    return identity;
  };

  const beforeRead = verify(expected);
  const { folder, agentDir } = beforeRead;
  const store = new ProjectTrustStore(agentDir);
  const before = store.getEntry(folder);
  const saved = before?.decision === true;
  const exact = saved && before.path === folder;
  const identity = verify([beforeRead], true);
  if (!grant || exact) return { folder, saved, exact, identity };

  dispatched = true;
  store.set(folder, true);
  const afterWrite = verify([identity], true);
  const entry = store.getEntry(folder);
  if (entry?.decision !== true || entry.path !== folder) throw 'unconfirmed';
  return { folder, saved: true, exact: true, identity: verify([afterWrite]) };
}

let result;
try {
  request = JSON.parse(input);
  result = await execute();
} catch (error) {
  result = {
    kind: dispatched
      ? 'unconfirmed'
      : failures.has(error)
        ? error
        : 'unavailable',
  };
}
const prefix = /^[a-f0-9-]{36}$/.test(request?.nonce)
  ? `\n__TAU_PROJECT_TRUST_${request.nonce}__`
  : '';
process.stdout.write(`${prefix}${JSON.stringify(result)}\n`);

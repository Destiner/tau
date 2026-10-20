import { createHash } from 'node:crypto';
import {
  chmodSync,
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { verifyBinary } from '../../gpui/scripts/package';
import { prepareCandidate, publishExclusive, verifyCandidate } from './bundle';
import { helperDiagnostic } from './keychain-diagnostics';

export type Expected = { version: string; commit: string };
export type Request = {
  args: string[];
  env: Record<string, string>;
  stdin?: string;
  timeout: number;
};
export type Result = { status: number; stdout: string; stderr: string };
export type Runner = (request: Request) => Result;
const bundleId = 'com.destiner.tau';
const limit = 4 * 1024 * 1024;
export class ReleaseError extends Error {}
function fail(label: string): never {
  throw new ReleaseError(label);
}
export function childEnvironment(
  source: NodeJS.ProcessEnv = process.env,
): Record<string, string> {
  const env: Record<string, string> = {};
  for (const key of [
    'PATH',
    'HOME',
    'TMPDIR',
    'SDKROOT',
    'DEVELOPER_DIR',
    'CARGO_HOME',
    'RUSTUP_HOME',
  ])
    if (source[key]) env[key] = source[key];
  env.LANG = 'en_US.UTF-8';
  env.CARGO_BUILD_JOBS = '2';
  if (source.CARGO_NET_OFFLINE === 'true') env.CARGO_NET_OFFLINE = 'true';
  env.MACOSX_DEPLOYMENT_TARGET = '15.0';
  return env;
}
export const systemRunner: Runner = (request) => {
  try {
    const result = Bun.spawnSync(request.args, {
      env: request.env,
      stdin:
        request.stdin === undefined ? undefined : Buffer.from(request.stdin),
      stdout: 'pipe',
      stderr: 'pipe',
      timeout: request.timeout,
      maxBuffer: limit,
    });
    return {
      status: result.exitCode,
      stdout: result.stdout.toString(),
      stderr: result.stderr.toString(),
    };
  } catch {
    return { status: -1, stdout: '', stderr: '' };
  }
};
function executor(runner: Runner) {
  return (
    args: string[],
    label = 'Tool operation failed',
    stdin?: string,
    timeout = 120_000,
    allowFailure = false,
  ): Result => {
    let result: Result;
    try {
      result = runner({ args, stdin, env: childEnvironment(), timeout });
    } catch {
      fail(label);
    }
    if (
      (!allowFailure && result.status !== 0) ||
      Buffer.byteLength(result.stdout) > limit ||
      Buffer.byteLength(result.stderr) > limit
    )
      fail(label);
    return result;
  };
}
function expected(value: Expected): void {
  if (
    !/^\d+\.\d+\.\d+$/.test(value.version) ||
    !/^[a-f0-9]{40}$/.test(value.commit)
  )
    fail('Invalid expected release');
}
function safe(path: string): void {
  if (
    !isAbsolute(path) ||
    resolve(path) !== path ||
    path === '/' ||
    /^\/(Applications|System|Library|Users\/[^/]+\/Applications)(\/|$)/.test(
      path,
    )
  )
    fail('Unsafe path');
  let current = path;
  while (!existsSync(current)) {
    try {
      lstatSync(current);
      fail('Unsafe dangling path');
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    }
    current = dirname(current);
  }
  if (realpathSync(current) !== current) fail('Path aliases forbidden');
}
function fresh(path: string): void {
  safe(path);
  try {
    lstatSync(path);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return;
    throw error;
  }
  fail('Destination already exists');
}
function hash(path: string): string {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}
function json(path: string): unknown {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    fail('Invalid metadata');
  }
}
function parse(text: string): Record<string, unknown> {
  try {
    const v: unknown = JSON.parse(text);
    if (!v || typeof v !== 'object' || Array.isArray(v))
      fail('Invalid tool metadata');
    return v as Record<string, unknown>;
  } catch {
    fail('Invalid tool metadata');
  }
}
function exact(directory: string, names: string[]): void {
  if (
    !lstatSync(directory).isDirectory() ||
    readdirSync(directory).sort().join('\n') !== [...names].sort().join('\n')
  )
    fail('Unexpected artifact layout');
}
function regular(path: string): void {
  if (!lstatSync(path).isFile() || lstatSync(path).nlink !== 1)
    fail('Unsafe artifact file');
}
interface Owner {
  schema: 1;
  path: string;
  uid: number;
  dev: number;
  ino: number;
  mountPending: boolean;
}
const marker = '.tau-release-owner.json';
function record(work: string, owner: Owner): void {
  writeFileSync(join(work, marker), JSON.stringify(owner), { mode: 0o600 });
}
function createWork(work: string): Owner {
  fresh(work);
  mkdirSync(work, { mode: 0o700 });
  const s = statSync(work);
  const owner: Owner = {
    schema: 1,
    path: work,
    uid: s.uid,
    dev: s.dev,
    ino: s.ino,
    mountPending: false,
  };
  record(work, owner);
  return owner;
}
function readOwner(work: string): Owner {
  safe(work);
  const s = lstatSync(work);
  const m = join(work, marker);
  regular(m);
  const owner = json(m) as Owner;
  if (
    !s.isDirectory() ||
    (s.mode & 0o777) !== 0o700 ||
    (lstatSync(m).mode & 0o777) !== 0o600 ||
    owner.schema !== 1 ||
    owner.path !== work ||
    owner.uid !== process.getuid?.() ||
    s.uid !== owner.uid ||
    s.dev !== owner.dev ||
    s.ino !== owner.ino ||
    typeof owner.mountPending !== 'boolean'
  )
    fail('Unowned work directory');
  return owner;
}
export function cleanup(
  work: string,
  helper: string | undefined,
  runner: Runner = systemRunner,
): void {
  const owner = readOwner(work);
  const exec = executor(runner);
  if (owner.mountPending) {
    exec(
      ['/usr/bin/hdiutil', 'detach', join(work, 'mount')],
      'Owned mount detach failed',
    );
    owner.mountPending = false;
    record(work, owner);
  }
  if (existsSync(join(work, 'keychain'))) {
    if (!helper) fail('Keychain cleanup requires helper');
    safe(helper);
    exec([helper, 'remove', join(work, 'keychain')], 'Keychain cleanup failed');
    if (existsSync(join(work, 'keychain'))) fail('Keychain cleanup incomplete');
  }
  rmSync(work, { recursive: true });
}
export function prepare(
  options: Expected & { output: string; targetDir?: string },
  runner: Runner = systemRunner,
): void {
  const exec = executor(runner);
  try {
    prepareCandidate(
      options,
      (args) =>
        exec(args, 'Candidate preparation failed', undefined, 3_600_000).stdout,
    );
  } catch {
    fail('Candidate preparation failed');
  }
}
export function verifyInput(
  input: string,
  value: Expected,
  runner: Runner = systemRunner,
): void {
  expected(value);
  safe(input);
  const exec = executor(runner);
  try {
    verifyCandidate(
      input,
      value,
      (args) => exec(args, 'Candidate inspection failed').stdout,
    );
  } catch {
    fail('Unsigned candidate verification failed');
  }
}
function signed(
  path: string,
  team: string,
  app: boolean,
  exec: ReturnType<typeof executor>,
): void {
  exec(
    [
      '/usr/bin/codesign',
      '--verify',
      '--deep',
      '--strict',
      '--verbose=2',
      '--test-requirement',
      `anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = "${team}" and identifier "${bundleId}"`,
      path,
    ],
    'Signature verification failed',
  );
  const result = exec(
    ['/usr/bin/codesign', '--display', '--verbose=4', path],
    'Signature metadata failed',
  );
  const text = result.stderr + '\n' + result.stdout;
  const authority = text.match(
    /^Authority=Developer ID Application: .+ \(([A-Z0-9]{10})\)$/m,
  );
  if (
    authority?.[1] !== team ||
    !text.includes(`TeamIdentifier=${team}\n`) ||
    !text.includes(`Identifier=${bundleId}\n`) ||
    !/^Timestamp=.+$/m.test(text) ||
    !/flags=.*\bruntime\b/.test(text) ||
    /Signature=adhoc/.test(text)
  )
    fail('Developer ID signature policy failed');
  if (app) {
    const entitlements = exec(
      ['/usr/bin/codesign', '--display', '--entitlements', ':-', path],
      'Entitlement inspection failed',
    ).stdout.trim();
    if (entitlements) {
      const data = parse(
        exec(
          ['/usr/bin/plutil', '-convert', 'json', '-o', '-', '-'],
          'Entitlement parsing failed',
          entitlements,
        ).stdout,
      );
      if (Object.keys(data).length) fail('Release entitlements forbidden');
    }
  }
}
function appTree(path: string): void {
  const stat = lstatSync(path);
  if (stat.isDirectory()) {
    for (const name of readdirSync(path)) appTree(join(path, name));
  } else if (!stat.isFile() || stat.nlink !== 1) fail('Unsafe signed app tree');
}
function inspectApp(
  app: string,
  value: Expected,
  team: string,
  exec: ReturnType<typeof executor>,
): void {
  appTree(app);
  const info = parse(
    exec(
      [
        '/usr/bin/plutil',
        '-convert',
        'json',
        '-o',
        '-',
        join(app, 'Contents/Info.plist'),
      ],
      'Bundle metadata failed',
    ).stdout,
  );
  if (
    info.CFBundleIdentifier !== bundleId ||
    info.CFBundleShortVersionString !== value.version ||
    info.CFBundleVersion !== value.version ||
    info.LSMinimumSystemVersion !== '15.0' ||
    info.CFBundleExecutable !== 'tau'
  )
    fail('Signed bundle metadata mismatch');
  const provenance = json(
    join(app, 'Contents/Resources/tau-build.json'),
  ) as Record<string, unknown>;
  if (
    provenance.version !== value.version ||
    provenance.commit !== value.commit ||
    provenance.bundleID !== bundleId ||
    provenance.target !== 'aarch64-apple-darwin' ||
    provenance.minimumOS !== '15.0'
  )
    fail('Signed provenance mismatch');
  const binary = join(app, 'Contents/MacOS/tau');
  if (
    (lstatSync(binary).mode & 0o777) !== 0o755 ||
    verifyBinary(
      binary,
      (args) => exec(args, 'Binary inspection failed').stdout,
    ) !== 'arm64'
  )
    fail('Signed architecture or executable mode mismatch');
  signed(app, team, true, exec);
  exec(
    ['/usr/bin/xcrun', 'stapler', 'validate', app],
    'App ticket validation failed',
  );
  exec(
    ['/usr/sbin/spctl', '--assess', '--type', 'execute', '--verbose=2', app],
    'Gatekeeper assessment failed',
  );
}
const uuid =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
function notarize(
  path: string,
  key: string,
  keyId: string,
  issuer: string,
  exec: ReturnType<typeof executor>,
): string {
  const result = exec(
    [
      '/usr/bin/xcrun',
      'notarytool',
      'submit',
      path,
      '--key',
      key,
      '--key-id',
      keyId,
      '--issuer',
      issuer,
      '--wait',
      '--timeout',
      '20m',
      '--output-format',
      'json',
    ],
    'Notarization command failed or timed out',
    undefined,
    1_260_000,
    true,
  );
  let receipt: Record<string, unknown>;
  try {
    receipt = parse(result.stdout);
  } catch {
    fail(
      result.status !== 0
        ? 'Notarization command failed or timed out'
        : 'Invalid notarization receipt',
    );
  }
  if (typeof receipt.id !== 'string' || !uuid.test(receipt.id))
    fail(
      result.status !== 0
        ? 'Notarization command failed or timed out'
        : 'Invalid notarization receipt',
    );
  if (result.status !== 0 || receipt.status !== 'Accepted')
    fail(`Notarization not accepted (${receipt.id})`);
  return receipt.id;
}
interface Manifest extends Expected {
  schemaVersion: 1;
  bundleId: string;
  platform: string;
  architecture: string;
  minimumSystemVersion: string;
  teamId: string;
  candidateManifestSha256: string;
  appRequestId: string;
  dmgRequestId: string;
  artifact: { name: string; sha256: string; size: number };
}
function artifactName(value: Expected): string {
  return `tau-${value.version}-apple-silicon.dmg`;
}
function mounted(
  dmg: string,
  work: string,
  value: Expected,
  team: string,
  runner: Runner,
): void {
  const exec = executor(runner);
  const owner = readOwner(work);
  const mount = join(work, 'mount');
  mkdirSync(mount, { recursive: true, mode: 0o700 });
  owner.mountPending = true;
  record(work, owner);
  try {
    exec(
      [
        '/usr/bin/hdiutil',
        'attach',
        '-readonly',
        '-nobrowse',
        '-noautoopen',
        '-mountpoint',
        mount,
        dmg,
      ],
      'DMG attachment failed',
    );
    exact(mount, ['Tau.app', 'Applications']);
    if (
      !lstatSync(join(mount, 'Tau.app')).isDirectory() ||
      !lstatSync(join(mount, 'Applications')).isSymbolicLink() ||
      readlinkSync(join(mount, 'Applications')) !== '/Applications'
    )
      fail('Invalid drag-install layout');
    inspectApp(join(mount, 'Tau.app'), value, team, exec);
  } finally {
    exec(['/usr/bin/hdiutil', 'detach', mount], 'Owned mount detach failed');
    owner.mountPending = false;
    record(work, owner);
  }
}
function verifyFinal(
  input: string,
  work: string,
  value: Expected,
  runner: Runner,
): Manifest {
  expected(value);
  safe(input);
  const name = artifactName(value);
  exact(input, [name, 'manifest.json', 'SHA256SUMS']);
  for (const file of [name, 'manifest.json', 'SHA256SUMS'])
    regular(join(input, file));
  const manifest = json(join(input, 'manifest.json')) as Manifest;
  if (
    !manifest ||
    typeof manifest !== 'object' ||
    Object.keys(manifest).sort().join(',') !==
      [
        'schemaVersion',
        'version',
        'commit',
        'bundleId',
        'platform',
        'architecture',
        'minimumSystemVersion',
        'teamId',
        'candidateManifestSha256',
        'appRequestId',
        'dmgRequestId',
        'artifact',
      ]
        .sort()
        .join(',') ||
    !manifest.artifact ||
    Object.keys(manifest.artifact).sort().join(',') !== 'name,sha256,size'
  )
    fail('Invalid final manifest schema');
  if (
    manifest.schemaVersion !== 1 ||
    manifest.version !== value.version ||
    manifest.commit !== value.commit ||
    manifest.bundleId !== bundleId ||
    manifest.platform !== 'macOS' ||
    manifest.architecture !== 'arm64' ||
    manifest.minimumSystemVersion !== '15.0' ||
    !/^[A-Z0-9]{10}$/.test(manifest.teamId) ||
    !/^[a-f0-9]{64}$/.test(manifest.candidateManifestSha256) ||
    !uuid.test(manifest.appRequestId) ||
    !uuid.test(manifest.dmgRequestId) ||
    manifest.artifact?.name !== name ||
    manifest.artifact.sha256 !== hash(join(input, name)) ||
    manifest.artifact.size !== statSync(join(input, name)).size
  )
    fail('Final manifest integrity failed');
  const sums = `${hash(join(input, name))}  ${name}\n${hash(join(input, 'manifest.json'))}  manifest.json\n`;
  if (readFileSync(join(input, 'SHA256SUMS'), 'utf8') !== sums)
    fail('Final checksum integrity failed');
  const exec = executor(runner);
  signed(join(input, name), manifest.teamId, false, exec);
  exec(
    ['/usr/bin/xcrun', 'stapler', 'validate', join(input, name)],
    'DMG ticket validation failed',
  );
  exec(
    [
      '/usr/sbin/spctl',
      '--assess',
      '--type',
      'open',
      '--context',
      'context:primary-signature',
      '--verbose=2',
      join(input, name),
    ],
    'DMG Gatekeeper assessment failed',
  );
  mounted(join(input, name), work, value, manifest.teamId, runner);
  return manifest;
}
export function verify(
  options: Expected & { input: string; work: string },
  runner: Runner = systemRunner,
): void {
  expected(options);
  safe(options.input);
  fresh(options.work);
  if (
    options.input === options.work ||
    options.work.startsWith(options.input + '/') ||
    options.input.startsWith(options.work + '/')
  )
    fail('Overlapping verification paths');
  createWork(options.work);
  try {
    verifyFinal(options.input, options.work, options, runner);
  } finally {
    cleanup(options.work, undefined, runner);
  }
}
export function sign(
  options: Expected & {
    input: string;
    output: string;
    work: string;
    keychainHelper: string;
  },
  runner: Runner = systemRunner,
  credentials: () => NodeJS.ProcessEnv = () => process.env,
): Manifest {
  verifyInput(options.input, options, runner);
  fresh(options.output);
  fresh(options.work);
  safe(options.keychainHelper);
  regular(options.keychainHelper);
  const paths = [options.input, options.output, options.work];
  if (
    paths.some((a, i) =>
      paths.some((b, j) => i !== j && (a === b || a.startsWith(b + '/'))),
    )
  )
    fail('Overlapping release paths');
  createWork(options.work);
  const exec = executor(runner);
  try {
    const payload = join(options.work, 'payload');
    mkdirSync(payload);
    const app = join(payload, 'Tau.app');
    cpSync(join(options.input, 'Tau.app'), app, {
      recursive: true,
      dereference: false,
    });
    cpSync(join(options.input, 'build.json'), join(payload, 'build.json'));
    verifyInput(payload, options, runner);
    const candidateManifestSha256 = hash(join(payload, 'build.json'));
    rmSync(join(payload, 'build.json'));
    chmodSync(join(app, 'Contents/MacOS/tau'), 0o755);
    const source = credentials();
    const certificate = source.APPLE_CERTIFICATE;
    const password = source.APPLE_CERTIFICATE_PASSWORD;
    const privateKey = source.APPLE_API_PRIVATE_KEY;
    const keyId = source.APPLE_API_KEY;
    const issuer = source.APPLE_API_ISSUER;
    if (
      !certificate ||
      password === undefined ||
      !privateKey ||
      !keyId ||
      !/^[A-Z0-9]{10}$/.test(keyId) ||
      !issuer ||
      !uuid.test(issuer)
    )
      fail('Missing or invalid signing credentials');
    const key = join(options.work, 'AuthKey.p8');
    writeFileSync(key, privateKey, { mode: 0o600, flag: 'wx' });
    const imported = exec(
      [options.keychainHelper, 'create', join(options.work, 'keychain')],
      'Signing identity import failed',
      JSON.stringify({ certificate, password }),
      120_000,
      true,
    );
    if (imported.status !== 0) {
      const detail = helperDiagnostic(imported.stderr);
      fail(`Signing identity import failed${detail ? `: ${detail}` : ''}`);
    }
    const identity = parse(imported.stdout);
    if (
      typeof identity.identity !== 'string' ||
      !/^[A-F0-9]{40}$/.test(identity.identity) ||
      typeof identity.teamId !== 'string' ||
      !/^[A-Z0-9]{10}$/.test(identity.teamId) ||
      typeof identity.name !== 'string' ||
      !identity.name.startsWith('Developer ID Application: ') ||
      !identity.name.endsWith(` (${identity.teamId})`) ||
      identity.keychain !== join(options.work, 'keychain/signing.keychain-db')
    )
      fail('Developer ID Application identity required');
    exec(
      [options.keychainHelper, 'activate', join(options.work, 'keychain')],
      'Signing keychain activation failed',
    );
    const codeSign = (path: string): Result =>
      exec(
        [
          '/usr/bin/codesign',
          '--force',
          '--sign',
          identity.identity as string,
          '--keychain',
          identity.keychain as string,
          '--identifier',
          bundleId,
          '--options',
          'runtime',
          '--timestamp',
          path,
        ],
        'Developer ID signing failed',
      );
    codeSign(app);
    signed(app, identity.teamId, true, exec);
    const zip = join(options.work, 'transport.zip');
    exec(
      ['/usr/bin/ditto', '-c', '-k', '--keepParent', app, zip],
      'Notarization transport failed',
    );
    const appRequestId = notarize(zip, key, keyId, issuer, exec);
    exec(['/usr/bin/xcrun', 'stapler', 'staple', app], 'App stapling failed');
    inspectApp(app, options, identity.teamId, exec);
    symlinkSync('/Applications', join(payload, 'Applications'));
    const stage = join(options.work, 'final');
    mkdirSync(stage);
    const name = artifactName(options);
    const dmg = join(stage, name);
    exec(
      [
        '/usr/bin/hdiutil',
        'create',
        '-fs',
        'HFS+',
        '-format',
        'UDZO',
        '-volname',
        'Tau',
        '-srcfolder',
        payload,
        dmg,
      ],
      'DMG creation failed',
    );
    codeSign(dmg);
    signed(dmg, identity.teamId, false, exec);
    const dmgRequestId = notarize(dmg, key, keyId, issuer, exec);
    exec(['/usr/bin/xcrun', 'stapler', 'staple', dmg], 'DMG stapling failed');
    const manifest: Manifest = {
      schemaVersion: 1,
      version: options.version,
      commit: options.commit,
      bundleId,
      platform: 'macOS',
      architecture: 'arm64',
      minimumSystemVersion: '15.0',
      teamId: identity.teamId,
      candidateManifestSha256,
      appRequestId,
      dmgRequestId,
      artifact: { name, sha256: hash(dmg), size: statSync(dmg).size },
    };
    writeFileSync(
      join(stage, 'manifest.json'),
      JSON.stringify(manifest, null, 2) + '\n',
    );
    writeFileSync(
      join(stage, 'SHA256SUMS'),
      `${hash(dmg)}  ${name}\n${hash(join(stage, 'manifest.json'))}  manifest.json\n`,
    );
    verifyFinal(stage, options.work, options, runner);
    // Remove credentials and the keychain before publication; failure must not publish.
    exec(
      [options.keychainHelper, 'remove', join(options.work, 'keychain')],
      'Keychain cleanup failed',
    );
    if (existsSync(join(options.work, 'keychain')))
      fail('Keychain cleanup incomplete');
    rmSync(key);
    publishExclusive(stage, options.output);
    return manifest;
  } finally {
    cleanup(options.work, options.keychainHelper, runner);
  }
}

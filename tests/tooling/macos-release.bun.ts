import { afterAll, expect, test } from 'bun:test';
import { createHash } from 'node:crypto';
import {
  chmodSync,
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { plist } from '../../gpui/scripts/package';
import {
  childEnvironment,
  cleanup,
  sign,
  verify,
  verifyInput,
  type Request,
  type Result,
  type Runner,
} from '../../scripts/macos/release';

const temp = realpathSync(mkdtempSync(join(tmpdir(), 'tau-release-tests-')));
const expected = { version: '0.3.0', commit: 'a'.repeat(40) };
const team = 'ABCDE12345';
const receipt = '12345678-abcd-1234-abcd-123456789012';
const secrets = {
  APPLE_CERTIFICATE: 'synthetic-certificate',
  APPLE_CERTIFICATE_PASSWORD: 'synthetic-password',
  APPLE_API_PRIVATE_KEY: 'synthetic-private-key',
  APPLE_API_KEY: 'PUBLIC1234',
  APPLE_API_ISSUER: receipt,
};
let sequence = 0;
function put(path: string, value: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, value);
}
function sha(path: string): string {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}
interface State {
  active: boolean;
  identity: string;
  signedTeam: string;
  version: string;
  arch: string;
  minimum: string;
  mountedMinimum: string;
  status: string;
  id: string;
  entitlements: string;
  runtime: boolean;
  timestamp: boolean;
  codeStatus: number;
  fail: (request: Request) => boolean;
  layout: string;
  detachFails: boolean;
}
function fixture(): {
  options: Parameters<typeof sign>[0];
  commands: Request[];
  runner: Runner;
  state: State;
  app: string;
  snapshot: string;
} {
  const root = join(temp, String(sequence++));
  mkdirSync(root);
  const input = join(root, 'unsigned');
  const app = join(input, 'Tau.app');
  const provenance = {
    version: expected.version,
    commit: expected.commit,
    bundleID: 'com.destiner.tau',
    target: 'aarch64-apple-darwin',
    minimumOS: '15.0',
  };
  const contents: Record<string, string> = {
    'Contents/MacOS/tau': 'fake Mach-O',
    'Contents/Info.plist': plist(expected.version, false, 'production'),
    'Contents/Resources/tau-build.json':
      JSON.stringify(provenance, null, 2) + '\n',
  };
  for (const [name, content] of Object.entries(contents))
    put(join(app, name), content);
  chmodSync(join(app, 'Contents/MacOS/tau'), 0o755);
  const manifest = {
    schemaVersion: 1,
    ...expected,
    bundleId: 'com.destiner.tau',
    architecture: 'arm64',
    minimumSystemVersion: '15.0',
    files: Object.keys(contents).map((name) => ({
      path: `Tau.app/${name}`,
      sha256: sha(join(app, name)),
      size: lstatSync(join(app, name)).size,
    })),
  };
  put(join(input, 'build.json'), JSON.stringify(manifest));
  const helper = join(root, 'helper');
  put(helper, 'synthetic helper, never executed');
  const options = {
    ...expected,
    input,
    output: join(root, 'final'),
    work: join(root, 'work'),
    keychainHelper: helper,
  };
  const commands: Request[] = [];
  const state: State = {
    active: false,
    identity: 'Developer ID Application: Example (ABCDE12345)',
    signedTeam: team,
    version: expected.version,
    arch: 'arm64',
    minimum: '15.0',
    mountedMinimum: '15.0',
    status: 'Accepted',
    id: receipt,
    entitlements: '',
    runtime: true,
    timestamp: true,
    codeStatus: 0,
    fail: (): boolean => false,
    layout: 'normal',
    detachFails: false,
  };
  const snapshot = join(root, 'image');
  const runner: Runner = (request): Result => {
    commands.push(request);
    const a = request.args;
    const tool = a[0]!;
    const ok = (stdout = '', stderr = ''): Result => ({
      status: 0,
      stdout,
      stderr,
    });
    if (state.fail(request))
      return {
        status: -1,
        stdout: secrets.APPLE_API_PRIVATE_KEY,
        stderr: secrets.APPLE_CERTIFICATE_PASSWORD,
      };
    if (tool === helper) {
      const directory = a[2]!;
      if (a[1] === 'create') {
        expect(
          lstatSync(join(options.work, 'payload/Tau.app/Contents/MacOS/tau'))
            .ino,
        ).not.toBe(lstatSync(join(app, 'Contents/MacOS/tau')).ino);
        expect(lstatSync(options.work).mode & 0o777).toBe(0o700);
        mkdirSync(directory, { mode: 0o700 });
        return ok(
          JSON.stringify({
            keychain: join(directory, 'signing.keychain-db'),
            identity: 'A'.repeat(40),
            name: state.identity,
            teamId: team,
          }),
        );
      }
      if (a[1] === 'activate') {
        expect(request.stdin).toBeUndefined();
        expect(existsSync(directory)).toBe(true);
        state.active = true;
        return ok();
      }
      expect(a[1]).toBe('remove');
      state.active = false;
      rmSync(directory, { recursive: true });
      return ok();
    }
    if (tool === '/usr/bin/file') return ok('Mach-O 64-bit executable arm64');
    if (tool === '/usr/bin/lipo') return ok(state.arch);
    if (tool === '/usr/bin/otool')
      return ok(
        a[1] === '-L'
          ? 'tau:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1)\n'
          : `Load command 1\n cmd LC_BUILD_VERSION\n cmdsize 32\n platform 1\n minos ${a[2]!.includes('/mount/') ? state.mountedMinimum : state.minimum}\n sdk 26.0\n`,
      );
    if (tool === '/usr/bin/plutil') {
      if (a[1] === '-lint') return ok('OK');
      if (a.at(-1) === '-') return ok(state.entitlements);
      return ok(
        JSON.stringify({
          CFBundleIdentifier: 'com.destiner.tau',
          CFBundleVersion: state.version,
          CFBundleShortVersionString: state.version,
          CFBundleExecutable: 'tau',
          LSMinimumSystemVersion: '15.0',
        }),
      );
    }
    if (tool === '/usr/bin/codesign') {
      if (a.includes('--sign')) expect(state.active).toBe(true);
      if (a.includes('--entitlements'))
        return ok(state.entitlements ? '<plist/>' : '');
      if (a.includes('--display'))
        return ok(
          '',
          `Identifier=com.destiner.tau\nAuthority=Developer ID Application: Example (${state.signedTeam})\nTeamIdentifier=${state.signedTeam}\n${state.timestamp ? 'Timestamp=Jan 1 2026\n' : ''}flags=0x10000(${state.runtime ? 'runtime' : 'none'})\n`,
        );
      return ok();
    }
    if (tool === '/usr/bin/ditto') {
      put(a.at(-1)!, 'private zip');
      return ok();
    }
    if (tool === '/usr/bin/xcrun') {
      if (a[1] === 'notarytool') {
        const key = a[a.indexOf('--key') + 1]!;
        expect(lstatSync(key).mode & 0o777).toBe(0o600);
        expect(readFileSync(key, 'utf8')).toBe(secrets.APPLE_API_PRIVATE_KEY);
        return {
          status: state.codeStatus,
          stdout: JSON.stringify({ status: state.status, id: state.id }),
          stderr: '',
        };
      }
      return ok();
    }
    if (tool === '/usr/sbin/spctl') return ok();
    if (tool === '/usr/bin/hdiutil') {
      if (a[1] === 'create') {
        put(a.at(-1)!, 'synthetic-dmg');
        cpSync(a[a.indexOf('-srcfolder') + 1]!, snapshot, {
          recursive: true,
          dereference: false,
        });
        return ok();
      }
      if (a[1] === 'attach') {
        const mount = a[a.indexOf('-mountpoint') + 1]!;
        cpSync(snapshot, mount, { recursive: true, dereference: false });
        if (state.layout === 'extra') put(join(mount, 'extra'), 'bad');
        return ok();
      }
      if (a[1] === 'detach') {
        if (state.detachFails)
          return { status: 1, stdout: '', stderr: 'raw secret error' };
        rmSync(a[2]!, { recursive: true });
        return ok();
      }
    }
    throw new Error('Unexpected fake command');
  };
  return { options, commands, runner, state, app, snapshot };
}
afterAll(() => rmSync(temp, { recursive: true, force: true }));

test('full order, copies input, closed output, credential isolation and durable verification', () => {
  const f = fixture();
  const before = sha(join(f.app, 'Contents/MacOS/tau'));
  const result = sign(f.options, f.runner, () => secrets);
  expect(result.appRequestId).toBe(receipt);
  expect(result.dmgRequestId).toBe(receipt);
  expect(result.candidateManifestSha256).toBe(
    sha(join(f.options.input, 'build.json')),
  );
  expect(sha(join(f.app, 'Contents/MacOS/tau'))).toBe(before);
  expect(existsSync(f.options.work)).toBe(false);
  expect(readdirSync(f.options.output).sort()).toEqual([
    'SHA256SUMS',
    'manifest.json',
    'tau-0.3.0-apple-silicon.dmg',
  ]);
  const lines = f.commands.map((r) => r.args.slice(0, 3).join(' '));
  const appStaple = lines.findIndex(
    (s) => s === '/usr/bin/xcrun stapler staple',
  );
  const dmgCreate = lines.findIndex((s) =>
    s.startsWith('/usr/bin/hdiutil create'),
  );
  expect(appStaple).toBeLessThan(dmgCreate);
  expect(f.commands.filter((r) => r.args[1] === 'notarytool')).toHaveLength(2);
  for (const request of f.commands) {
    for (const secret of [
      secrets.APPLE_CERTIFICATE,
      secrets.APPLE_CERTIFICATE_PASSWORD,
      secrets.APPLE_API_PRIVATE_KEY,
    ]) {
      expect(JSON.stringify(request.args)).not.toContain(secret);
      expect(JSON.stringify(request.env)).not.toContain(secret);
      expect(request.env.CARGO_BUILD_JOBS).toBe('2');
      expect(request.env.MACOSX_DEPLOYMENT_TARGET).toBe('15.0');
      expect(request.env).not.toHaveProperty('DEBUG');
      expect(request.env).not.toHaveProperty('RUSTFLAGS');
      expect(request.env).not.toHaveProperty('RUSTC_WRAPPER');
    }
    if (request.stdin) expect(request.args[0]).toBe(f.options.keychainHelper);
  }
  const finalContents = readdirSync(f.options.output)
    .map((name) => readFileSync(join(f.options.output, name), 'utf8'))
    .join('');
  expect(finalContents).not.toContain(f.options.work);
  for (const secret of [
    secrets.APPLE_CERTIFICATE,
    secrets.APPLE_CERTIFICATE_PASSWORD,
    secrets.APPLE_API_PRIVATE_KEY,
  ])
    expect(finalContents).not.toContain(secret);
  verify(
    {
      ...expected,
      input: f.options.output,
      work: join(dirname(f.options.work), 'verify-work'),
    },
    f.runner,
  );
  const signatureChecks = f.commands.filter(
    (r) => r.args[0] === '/usr/bin/codesign' && r.args[1] === '--verify',
  );
  expect(signatureChecks.some((r) => r.args.at(-1)!.endsWith('.app'))).toBe(
    true,
  );
  expect(signatureChecks.some((r) => r.args.at(-1)!.endsWith('.dmg'))).toBe(
    true,
  );
  for (const { args } of signatureChecks) {
    expect(args).toEqual([
      '/usr/bin/codesign',
      '--verify',
      '--deep',
      '--strict',
      '--verbose=2',
      '--test-requirement',
      '=anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = "ABCDE12345" and identifier "com.destiner.tau"',
      args.at(-1)!,
    ]);
  }
});

test('stale, tampered, extra and symlink input reject before reading credentials', () => {
  for (const change of ['stale', 'tamper', 'extra', 'symlink']) {
    const f = fixture();
    let consumed = false;
    if (change === 'stale') f.options.commit = 'b'.repeat(40);
    if (change === 'tamper') put(join(f.app, 'Contents/MacOS/tau'), 'tampered');
    if (change === 'extra') put(join(f.options.input, 'extra'), 'bad');
    if (change === 'symlink') {
      rmSync(join(f.app, 'Contents/MacOS/tau'));
      symlinkSync('/bin/sh', join(f.app, 'Contents/MacOS/tau'));
    }
    expect(() =>
      sign(f.options, f.runner, () => {
        consumed = true;
        return secrets;
      }),
    ).toThrow('Unsigned candidate');
    expect(consumed).toBe(false);
    expect(existsSync(f.options.work)).toBe(false);
  }
});

test('reject development identities, team/runtime/timestamp/version/architecture/entitlement mismatch', () => {
  for (const change of [
    'identity',
    'team',
    'runtime',
    'timestamp',
    'version',
    'architecture',
    'entitlements',
  ]) {
    const f = fixture();
    if (change === 'identity')
      f.state.identity = 'Apple Development: Example (ABCDE12345)';
    if (change === 'team') f.state.signedTeam = 'ZZZZZ12345';
    if (change === 'runtime') f.state.runtime = false;
    if (change === 'timestamp') f.state.timestamp = false;
    if (change === 'version') f.state.version = '0.2.1';
    if (change === 'architecture') f.state.arch = 'x86_64';
    if (change === 'entitlements')
      f.state.entitlements = '{"com.apple.security.get-task-allow":true}';
    expect(() => sign(f.options, f.runner, () => secrets)).toThrow();
    expect(existsSync(f.options.output)).toBe(false);
    expect(existsSync(f.options.work)).toBe(false);
  }
});

test('pending/rejected/malformed/nonzero/timeout notarization fail closed without retries or raw errors', () => {
  for (const change of [
    'pending',
    'rejected',
    'malformed',
    'nonzero',
    'timeout',
  ]) {
    const f = fixture();
    if (change === 'pending') f.state.status = 'In Progress';
    if (change === 'rejected') f.state.status = 'Invalid';
    if (change === 'malformed') f.state.id = 'bad-id';
    if (change === 'nonzero') f.state.codeStatus = 1;
    if (change === 'timeout')
      f.state.fail = (r): boolean => r.args[1] === 'notarytool';
    let message = '';
    try {
      sign(f.options, f.runner, () => secrets);
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).not.toBe('');
    expect(message).not.toContain(secrets.APPLE_CERTIFICATE_PASSWORD);
    if (change !== 'malformed' && change !== 'timeout')
      expect(message).toContain(receipt);
    expect(f.commands.filter((r) => r.args[1] === 'notarytool')).toHaveLength(
      1,
    );
    expect(existsSync(f.options.output)).toBe(false);
    expect(existsSync(f.options.work)).toBe(false);
  }
});

test('signature, ticket, assessment, mount-layout and attach failures clean only owned work', () => {
  for (const change of [
    'signature',
    'ticket',
    'assessment',
    'layout',
    'attach',
  ]) {
    const f = fixture();
    if (change === 'signature')
      f.state.fail = (r): boolean =>
        r.args[0] === '/usr/bin/codesign' && r.args[1] === '--verify';
    if (change === 'ticket')
      f.state.fail = (r): boolean =>
        r.args[1] === 'stapler' && r.args[2] === 'validate';
    if (change === 'assessment')
      f.state.fail = (r): boolean => r.args[0] === '/usr/sbin/spctl';
    if (change === 'layout') f.state.layout = 'extra';
    if (change === 'attach')
      f.state.fail = (r): boolean => r.args[1] === 'attach';
    expect(() => sign(f.options, f.runner, () => secrets)).toThrow();
    expect(existsSync(f.options.output)).toBe(false);
    expect(existsSync(f.options.work)).toBe(false);
    if (change === 'layout' || change === 'attach')
      expect(f.commands.some((r) => r.args[1] === 'detach')).toBe(true);
  }
});

test('failed detach retains owner and mount; always cleanup retries exact owned mount', () => {
  const f = fixture();
  f.state.detachFails = true;
  expect(() => sign(f.options, f.runner, () => secrets)).toThrow('detach');
  expect(existsSync(join(f.options.work, 'mount/Tau.app'))).toBe(true);
  expect(existsSync(join(f.options.work, '.tau-release-owner.json'))).toBe(
    true,
  );
  expect(existsSync(f.options.output)).toBe(false);
  f.state.detachFails = false;
  cleanup(f.options.work, f.options.keychainHelper, f.runner);
  expect(existsSync(f.options.work)).toBe(false);
  expect(
    f.commands
      .filter((r) => r.args[1] === 'detach')
      .every(
        (r) =>
          r.args.length === 3 && r.args[2] === join(f.options.work, 'mount'),
      ),
  ).toBe(true);
});

test('never overwrite, accept unsafe aliases, or clean an unowned directory', () => {
  const f = fixture();
  mkdirSync(f.options.output);
  put(join(f.options.output, 'sentinel'), 'keep');
  expect(() => sign(f.options, f.runner, () => secrets)).toThrow('exists');
  expect(readFileSync(join(f.options.output, 'sentinel'), 'utf8')).toBe('keep');
  expect(() =>
    cleanup(f.options.output, f.options.keychainHelper, f.runner),
  ).toThrow();
  const alias = join(dirname(f.options.output), 'alias');
  symlinkSync(f.options.input, alias);
  expect(() => verifyInput(alias, expected, f.runner)).toThrow('aliases');
  for (const work of [
    '/Applications/Tau.app',
    '/private/tmp/../bad',
    'relative',
  ])
    expect(() =>
      verify({ ...expected, input: f.options.output, work }, f.runner),
    ).toThrow();
  expect(f.commands).toHaveLength(5); // Initial unsigned inspection only.
});

test('durable verification recomputes hashes, closes file set and compares caller expectations', () => {
  for (const change of ['hash', 'extra', 'manifest', 'commit', 'version']) {
    const f = fixture();
    sign(f.options, f.runner, () => secrets);
    const manifestPath = join(f.options.output, 'manifest.json');
    if (change === 'hash')
      put(join(f.options.output, 'tau-0.3.0-apple-silicon.dmg'), 'tampered');
    if (change === 'extra') put(join(f.options.output, 'latest.json'), '{}');
    if (change === 'manifest') {
      const m = JSON.parse(readFileSync(manifestPath, 'utf8')) as Record<
        string,
        unknown
      >;
      m.teamId = 'ZZZZZ12345';
      put(manifestPath, JSON.stringify(m));
    }
    expect(() =>
      verify(
        {
          version: change === 'version' ? '0.4.0' : expected.version,
          commit: change === 'commit' ? 'b'.repeat(40) : expected.commit,
          input: f.options.output,
          work: join(dirname(f.options.work), 'verify-work'),
        },
        f.runner,
      ),
    ).toThrow();
  }
});

test('keychain cleanup failure prevents publication and preserves ownership for fallback', () => {
  const f = fixture();
  f.state.fail = (r): boolean =>
    r.args[0] === f.options.keychainHelper && r.args[1] === 'remove';
  expect(() => sign(f.options, f.runner, () => secrets)).toThrow(
    'Keychain cleanup failed',
  );
  expect(existsSync(f.options.output)).toBe(false);
  expect(existsSync(join(f.options.work, '.tau-release-owner.json'))).toBe(
    true,
  );
  f.state.fail = (): boolean => false;
  cleanup(f.options.work, f.options.keychainHelper, f.runner);
  expect(existsSync(f.options.work)).toBe(false);
});

test('publication race cannot replace even an empty directory', () => {
  const f = fixture();
  const runner: Runner = (request) => {
    const result = f.runner(request);
    if (
      request.args[0] === f.options.keychainHelper &&
      request.args[1] === 'remove'
    )
      mkdirSync(f.options.output);
    return result;
  };
  expect(() => sign(f.options, runner, () => secrets)).toThrow('exclusive');
  expect(readdirSync(f.options.output)).toEqual([]);
  expect(existsSync(f.options.work)).toBe(false);
});

test('recomputed manifest checksums cannot override mounted identity or caller commit', () => {
  for (const change of ['team', 'commit']) {
    const f = fixture();
    sign(f.options, f.runner, () => secrets);
    const path = join(f.options.output, 'manifest.json');
    const m = JSON.parse(readFileSync(path, 'utf8')) as Record<string, unknown>;
    m[change === 'team' ? 'teamId' : 'commit'] =
      change === 'team' ? 'ZZZZZ12345' : 'b'.repeat(40);
    put(path, JSON.stringify(m));
    const name = 'tau-0.3.0-apple-silicon.dmg';
    put(
      join(f.options.output, 'SHA256SUMS'),
      `${sha(join(f.options.output, name))}  ${name}\n${sha(path)}  manifest.json\n`,
    );
    expect(() =>
      verify(
        {
          ...expected,
          input: f.options.output,
          work: join(dirname(f.options.work), 'verify-work'),
        },
        f.runner,
      ),
    ).toThrow();
  }
});

test('copied ownership markers do not authorize cleanup', () => {
  const f = fixture();
  f.state.detachFails = true;
  expect(() => sign(f.options, f.runner, () => secrets)).toThrow();
  const foreign = join(dirname(f.options.work), 'foreign');
  mkdirSync(foreign, { mode: 0o700 });
  cpSync(
    join(f.options.work, '.tau-release-owner.json'),
    join(foreign, '.tau-release-owner.json'),
  );
  expect(() => cleanup(foreign, f.options.keychainHelper, f.runner)).toThrow(
    'Unowned',
  );
  expect(existsSync(foreign)).toBe(true);
  f.state.detachFails = false;
  cleanup(f.options.work, f.options.keychainHelper, f.runner);
});

test('incompatible unsigned deployment rejects before credentials; mounted binary is independently inspected', () => {
  for (const minimum of ['15.1', '26.0']) {
    const f = fixture();
    f.state.minimum = minimum;
    let reads = 0;
    expect(() =>
      sign(f.options, f.runner, () => {
        reads++;
        return secrets;
      }),
    ).toThrow();
    expect(reads).toBe(0);
    expect(existsSync(f.options.output)).toBe(false);
    expect(
      f.commands.some(
        (request) => request.args[0] === f.options.keychainHelper,
      ),
    ).toBe(false);
  }
  const f = fixture();
  sign(f.options, f.runner, () => secrets);
  f.state.mountedMinimum = '15.1';
  const work = join(dirname(f.options.work), 'deployment-verify');
  expect(() =>
    verify({ ...expected, input: f.options.output, work }, f.runner),
  ).toThrow();
  expect(
    f.commands.some(
      (request) =>
        request.args[0] === '/usr/bin/otool' &&
        request.args[1] === '-l' &&
        request.args[2] === join(work, 'mount/Tau.app/Contents/MacOS/tau'),
    ),
  ).toBe(true);
});

test('child environment is an allowlist, including build invocations', () => {
  expect(
    childEnvironment({
      PATH: '/usr/bin',
      HOME: '/owned',
      APPLE_CERTIFICATE: 'secret',
      AWS_SECRET_ACCESS_KEY: 'secret',
      NODE_OPTIONS: '--require evil',
      BASH_ENV: '/evil',
      CARGO_BUILD_JOBS: '128',
      MACOSX_DEPLOYMENT_TARGET: '26.0',
      RUSTFLAGS: '--cfg evil',
      RUSTC_WRAPPER: '/evil',
      RUSTC_WORKSPACE_WRAPPER: '/evil',
      DEBUG: '0',
    }),
  ).toEqual({
    PATH: '/usr/bin',
    HOME: '/owned',
    LANG: 'en_US.UTF-8',
    CARGO_BUILD_JOBS: '2',
    MACOSX_DEPLOYMENT_TARGET: '15.0',
  });
  expect(
    childEnvironment({ CARGO_NET_OFFLINE: 'true' }).CARGO_NET_OFFLINE,
  ).toBe('true');
  for (const offline of [undefined, 'false', 'invalid'])
    expect(
      childEnvironment({ CARGO_NET_OFFLINE: offline }).CARGO_NET_OFFLINE,
    ).toBeUndefined();
  for (const jobs of [undefined, '2', '128']) {
    const env = childEnvironment({ CARGO_BUILD_JOBS: jobs });
    expect(env.CARGO_BUILD_JOBS).toBe('2');
    expect(env.MACOSX_DEPLOYMENT_TARGET).toBe('15.0');
  }
});

test('owned membership lifetime has exact create/activate/sign/remove order', () => {
  const f = fixture();
  sign(f.options, f.runner, () => secrets);
  expect(
    f.commands.flatMap(({ args }) =>
      args[0] === f.options.keychainHelper
        ? [args[1]]
        : args[0] === '/usr/bin/codesign' && args.includes('--sign')
          ? ['sign']
          : [],
    ),
  ).toEqual(['create', 'activate', 'sign', 'sign', 'remove']);
  expect(f.state.active).toBe(false);
});

test('activation failures block signing, notarization and output; partial membership supports owned cleanup retry', () => {
  for (const partial of [false, true]) {
    const f = fixture();
    f.state.fail = ({ args }): boolean => {
      if (args[0] !== f.options.keychainHelper) return false;
      if (args[1] === 'activate') {
        f.state.active = partial;
        return true;
      }
      return partial && args[1] === 'remove';
    };
    try {
      sign(f.options, f.runner, () => secrets);
      throw new Error('Expected failure');
    } catch (error) {
      expect(String(error)).not.toContain(secrets.APPLE_CERTIFICATE_PASSWORD);
      expect(String(error)).not.toContain(secrets.APPLE_API_PRIVATE_KEY);
      expect(String(error)).not.toContain(f.options.work);
    }
    expect(
      f.commands.some(
        ({ args }) =>
          args[0] === '/usr/bin/codesign' || args[1] === 'notarytool',
      ),
    ).toBe(false);
    expect(existsSync(f.options.output)).toBe(false);
    expect(existsSync(f.options.work)).toBe(partial);
    expect(f.state.active).toBe(partial);
    f.state.fail = (): boolean => false;
    if (partial) cleanup(f.options.work, f.options.keychainHelper, f.runner);
    expect(f.state.active).toBe(false);
    expect(existsSync(f.options.work)).toBe(false);
  }
});

test('import failures expose only exact safe helper diagnostics and clean owned work', () => {
  const safe = [
    'Import PKCS12 failed (OSStatus -25257)',
    'Invalid base64 certificate',
    'Invalid ownership marker',
    'Writable directory ancestor',
  ];
  const rejected = [
    '',
    secrets.APPLE_CERTIFICATE_PASSWORD,
    `Invalid base64 certificate ${secrets.APPLE_CERTIFICATE}`,
    `Import PKCS12 failed (OSStatus -25257) ${secrets.APPLE_API_PRIVATE_KEY}`,
    `Invalid ownership marker\n${secrets.APPLE_CERTIFICATE_PASSWORD}`,
    'Invalid ownership marker\n\n',
    'Invalid ownership marker\r',
    'Invalid ownership marker\r\n',
    'Invalid ownership marker\0',
    'Invalid ownership marker\t',
    '\x1b[31mInvalid ownership marker',
    'x'.repeat(257),
    `Invalid ownership marker${' '.repeat(257)}`,
    ...['+1', '-0', '01', '-01', '1.0', '1e2', '2147483648', '-2147483649'].map(
      (status) => `Import PKCS12 failed (OSStatus ${status})`,
    ),
    'Unknown operation failed (OSStatus -50)',
  ];
  for (const [stderr, detail] of [
    ...safe.flatMap((line) => [
      [line, line],
      [`${line}\n`, line],
    ]),
    ...rejected.map((line) => [line, '']),
  ]) {
    const f = fixture();
    const runner: Runner = (request) => {
      const result = f.runner(request);
      if (
        request.args[0] !== f.options.keychainHelper ||
        request.args[1] !== 'create'
      )
        return result;
      expect(request.timeout).toBe(120_000);
      expect(request.stdin).toBe(
        JSON.stringify({
          certificate: secrets.APPLE_CERTIFICATE,
          password: secrets.APPLE_CERTIFICATE_PASSWORD,
        }),
      );
      return {
        status: 1,
        stdout: secrets.APPLE_API_PRIVATE_KEY,
        stderr: stderr!,
      };
    };
    let message = '';
    try {
      sign(f.options, runner, () => secrets);
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).toBe(
      `Signing identity import failed${detail ? `: ${detail}` : ''}`,
    );
    for (const secret of Object.values(secrets))
      expect(message).not.toContain(secret);
    expect(message).not.toContain(f.options.work);
    expect(message).not.toContain(f.options.keychainHelper);
    expect(
      f.commands.some(
        ({ args }) =>
          args[1] === 'activate' ||
          args[0] === '/usr/bin/codesign' ||
          args[1] === 'notarytool',
      ),
    ).toBe(false);
    expect(
      f.commands
        .filter(({ args }) => args[0] === f.options.keychainHelper)
        .map(({ args }) => args[1]),
    ).toEqual(['create', 'remove']);
    expect(f.state.active).toBe(false);
    expect(existsSync(f.options.output)).toBe(false);
    expect(existsSync(f.options.work)).toBe(false);
  }
});

test('import executor exceptions and buffer guards retain generic failures', () => {
  for (const mode of [
    'throw',
    'stdout-limit',
    'stderr-limit',
    'valid-stdout',
  ]) {
    const f = fixture();
    const runner: Runner = (request) => {
      const result = f.runner(request);
      if (
        request.args[0] !== f.options.keychainHelper ||
        request.args[1] !== 'create'
      )
        return result;
      if (mode === 'throw')
        throw new Error(
          `${request.args} ${request.stdin} ${secrets.APPLE_API_PRIVATE_KEY}`,
        );
      return {
        status: 1,
        stdout:
          mode === 'stdout-limit'
            ? 'x'.repeat(4 * 1024 * 1024 + 1)
            : result.stdout,
        stderr:
          mode === 'stderr-limit'
            ? 'x'.repeat(4 * 1024 * 1024 + 1)
            : mode === 'valid-stdout'
              ? secrets.APPLE_CERTIFICATE_PASSWORD
              : 'Invalid base64 certificate',
      };
    };
    let message = '';
    try {
      sign(f.options, runner, () => secrets);
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).toBe('Signing identity import failed');
    expect(
      f.commands.some(
        ({ args }) =>
          args[1] === 'activate' ||
          args[0] === '/usr/bin/codesign' ||
          args[1] === 'notarytool',
      ),
    ).toBe(false);
    expect(existsSync(f.options.output)).toBe(false);
    expect(existsSync(f.options.work)).toBe(false);
  }
});

test('invalid Developer ID metadata never activates membership', () => {
  const f = fixture();
  f.state.identity = 'Apple Development: Example (ABCDE12345)';
  expect(() => sign(f.options, f.runner, () => secrets)).toThrow(
    'Developer ID',
  );
  expect(f.commands.some(({ args }) => args[1] === 'activate')).toBe(false);
  expect(f.state.active).toBe(false);
});

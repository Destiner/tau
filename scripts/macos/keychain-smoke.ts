import { randomBytes } from 'node:crypto';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  rmdirSync,
  writeFileSync,
} from 'node:fs';
import { join, resolve } from 'node:path';
import { boundedInteger, helperDiagnostic } from './keychain-diagnostics';

export interface SmokeContext {
  githubActions?: string;
  runnerEnvironment?: string;
  platform: string;
  uid: number;
}
export function requireDisposable(context: SmokeContext): void {
  if (
    context.githubActions !== 'true' ||
    context.runnerEnvironment !== 'github-hosted' ||
    context.platform !== 'darwin' ||
    context.uid === 0
  ) {
    throw new Error(
      'Synthetic smoke requires a non-root disposable GitHub macOS runner',
    );
  }
}
export type SmokeRunner = (
  args: string[],
  stdin?: string,
) => { status: number; stdout: string; stderr?: string };

type Phase =
  | 'setup'
  | 'context'
  | 'snapshot'
  | 'createcertificate'
  | 'exportP12'
  | 'helpercreate'
  | 'metadata'
  | 'fingerprint'
  | 'activate'
  | 'sign'
  | 'verify'
  | 'remove'
  | 'wrongpassword'
  | 'cleanup';
type Category =
  | 'exception'
  | 'command'
  | 'mismatch'
  | 'missing-baseline'
  | 'unexpected-success'
  | 'owned-files';

function codesignDiagnostic(stderr: string | undefined): string | undefined {
  if (stderr === undefined || stderr.length > 16_384) return;
  const markers = [
    ['errSecInternalComponent', 'internal-component'],
    [
      'The specified item could not be found in the keychain',
      'identity-not-found',
    ],
    ['User interaction is not allowed', 'interaction-denied'],
    ['unable to build chain to self-signed root', 'chain-warning'],
    ['CSSMERR_TP_NOT_TRUSTED', 'untrusted-chain'],
    ['CSSMERR_TP_CERT_EXPIRED', 'expired-certificate'],
    [
      'resource fork, Finder information, or similar detritus not allowed',
      'resource-metadata',
    ],
    ['bundle format unrecognized, invalid, or unsuitable', 'invalid-format'],
    ['code object is not signed at all', 'unsigned-input'],
  ] as const;
  const labels = markers
    .filter(([marker]) => stderr.includes(marker))
    .map(([, label]) => label);
  return labels.length ? `codesign:${labels.join(',')}` : undefined;
}
function scopedIdentityDiagnostic(run: SmokeRunner, keychain: string): string {
  const summaries: string[] = [];
  for (const policy of ['basic', 'codesigning']) {
    try {
      const result = run([
        '/usr/bin/security',
        'find-identity',
        '-p',
        policy,
        keychain,
      ]);
      if (result.status !== 0 || result.stdout.length > 16_384) continue;
      const matches = /^\s*([0-9]+) identities found\s*$/m.exec(result.stdout);
      const valid = /^\s*([0-9]+) valid identities found\s*$/m.exec(
        result.stdout,
      );
      if (
        matches &&
        valid &&
        Number(matches[1]) <= 10 &&
        Number(valid[1]) <= 10
      )
        summaries.push(
          `scoped-${policy}=${Number(matches[1])}/${Number(valid[1])}`,
        );
    } catch {
      // A diagnostic lookup must not replace the original signing failure.
    }
  }
  return summaries.join(',');
}
const diagnostics = new WeakMap<object, string>();
function failure(
  phase: Phase,
  category: Category,
  status?: number,
  detail?: string,
): Error {
  const summary = `phase=${phase} category=${category} exitstatus=${status !== undefined && boundedInteger(status) ? status : 'unavailable'}${detail ? ` detail=${detail}` : ''}`;
  const error = new Error(summary);
  diagnostics.set(error, summary);
  return error;
}
// This is the only CLI error formatter; unknown errors are never stringified.
export function safeSmokeDiagnostic(error: unknown): string {
  return (
    (typeof error === 'object' && error !== null
      ? diagnostics.get(error)
      : undefined) ?? 'phase=setup category=exception exitstatus=unavailable'
  );
}
function at<T>(phase: Phase, body: () => T): T {
  try {
    return body();
  } catch (error) {
    if (typeof error === 'object' && error !== null && diagnostics.has(error))
      throw error;
    throw failure(phase, 'exception');
  }
}

// Never include child output or arguments in failures: openssl/helper may handle private material.
export function smoke(
  helper: string,
  parent: string,
  context: SmokeContext,
  run: SmokeRunner,
): void {
  at('context', () => requireDisposable(context));
  at('setup', () => {
    if (
      realpathSync(parent) !== parent ||
      realpathSync(helper) !== helper ||
      !resolve(helper).startsWith('/')
    )
      throw failure('setup', 'mismatch');
  });
  const root = at('setup', () => mkdtempSync(join(parent, 'tau-synthetic-')));
  const owned = join(root, 'identity');
  const wrong = join(root, 'wrong-password');
  const command = (phase: Phase, args: string[], stdin?: string): string =>
    at(phase, () => {
      const result = run(args, stdin);
      if (result.status !== 0) {
        const detail =
          args[0] === helper
            ? helperDiagnostic(result.stderr)
            : args[0] === '/usr/bin/codesign'
              ? codesignDiagnostic(result.stderr)
              : undefined;
        const keychainIndex = args.indexOf('--keychain');
        const identities =
          phase === 'sign' && keychainIndex >= 0
            ? scopedIdentityDiagnostic(run, args[keychainIndex + 1]!)
            : '';
        throw failure(
          phase,
          'command',
          result.status,
          [detail, identities].filter(Boolean).join(';'),
        );
      }
      return result.stdout;
    });
  const snapshot = (phase: Phase = 'snapshot'): string =>
    JSON.stringify([
      command(phase, ['/usr/bin/security', 'list-keychains', '-d', 'user']),
      command(phase, ['/usr/bin/security', 'default-keychain', '-d', 'user']),
    ]);
  let baseline: string | undefined;
  const assertRestored = (): void => {
    if (baseline === undefined) throw failure('cleanup', 'missing-baseline');
    if (snapshot('cleanup') !== baseline) throw failure('cleanup', 'mismatch');
  };
  let phase: Phase = 'setup';
  let original: unknown;
  const cleanupErrors: string[] = [];
  try {
    chmodSync(root, 0o700);
    phase = 'snapshot';
    baseline = snapshot();
    phase = 'createcertificate';
    const password = randomBytes(32).toString('hex');
    const key = join(root, 'key.pem');
    const cert = join(root, 'cert.pem');
    const p12 = join(root, 'synthetic.p12');
    const config = join(root, 'certificate.cnf');
    writeFileSync(
      config,
      '[req]\ndistinguished_name=subject\nx509_extensions=signing\n[subject]\n[signing]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=codeSigning\n',
      { mode: 0o600 },
    );
    command(
      'createcertificate',
      [
        '/usr/bin/openssl',
        'req',
        '-new',
        '-x509',
        '-newkey',
        'rsa:2048',
        '-sha256',
        '-days',
        '1',
        '-subj',
        '/CN=Tau Synthetic Smoke/OU=SYNTHETIC1',
        '-config',
        config,
        '-keyout',
        key,
        '-out',
        cert,
        '-passout',
        'stdin',
      ],
      `${password}\n`,
    );
    chmodSync(key, 0o600);
    phase = 'exportP12';
    command(
      'exportP12',
      [
        '/usr/bin/openssl',
        'pkcs12',
        '-export',
        '-inkey',
        key,
        '-in',
        cert,
        '-out',
        p12,
        '-passin',
        'stdin',
        '-passout',
        'stdin',
      ],
      `${password}\n${password}\n`,
    );
    chmodSync(p12, 0o600);
    const encoded = readFileSync(p12).toString('base64');
    const certificate = ` \n${encoded.match(/.{1,64}/g)!.join('\r\n')}\n `;
    const metadata = command(
      'helpercreate',
      [helper, 'create', owned],
      JSON.stringify({ certificate, password }),
    );
    phase = 'metadata';
    const identity = JSON.parse(metadata) as Record<string, string>;
    phase = 'fingerprint';
    const fingerprint = command('fingerprint', [
      '/usr/bin/openssl',
      'x509',
      '-in',
      cert,
      '-noout',
      '-fingerprint',
      '-sha1',
    ])
      .trim()
      .split('=')
      .at(-1)
      ?.replaceAll(':', '');
    if (!/^[A-F0-9]{40}$/.test(fingerprint ?? ''))
      throw failure('fingerprint', 'mismatch');
    phase = 'metadata';
    if (
      Object.keys(identity).sort().join(',') !==
        'identity,keychain,name,teamId' ||
      identity.keychain !== join(owned, 'signing.keychain-db') ||
      !/^[A-F0-9]{40}$/.test(identity.identity ?? '') ||
      identity.identity !== fingerprint ||
      identity.name !== 'Tau Synthetic Smoke' ||
      identity.teamId !== 'SYNTHETIC1'
    )
      throw failure('metadata', 'mismatch');
    if (snapshot() !== baseline) throw failure('snapshot', 'mismatch');
    phase = 'sign';
    const binary = join(root, 'harmless-mach-o');
    copyFileSync('/usr/bin/true', binary);
    chmodSync(binary, 0o700);
    {
      command('activate', [helper, 'activate', owned]);
      const defaultKeychain = command('snapshot', [
        '/usr/bin/security',
        'default-keychain',
        '-d',
        'user',
      ]);
      if (defaultKeychain !== (JSON.parse(baseline) as string[])[1])
        throw failure('snapshot', 'mismatch');
    }
    command('sign', [
      '/usr/bin/codesign',
      '--force',
      '--sign',
      identity.identity!,
      '--keychain',
      identity.keychain!,
      '--timestamp=none',
      binary,
    ]);
    command('verify', [
      '/usr/bin/codesign',
      '--verify',
      '--strict',
      '--test-requirement',
      `=certificate leaf = H"${fingerprint}"`,
      binary,
    ]);
    // Exit 3 distinguishes an unmet requirement from an invalid signature or invocation.
    at('verify', () => {
      const wrongFingerprint =
        (fingerprint![0] === 'A' ? 'B' : 'A') + fingerprint!.slice(1);
      const rejected = run([
        '/usr/bin/codesign',
        '--verify',
        '--strict',
        '--test-requirement',
        `=certificate leaf = H"${wrongFingerprint}"`,
        binary,
      ]);
      if (rejected.status !== 3)
        throw failure('verify', 'command', rejected.status);
    });
    command('remove', [helper, 'remove', owned]);
    if (snapshot() !== baseline) throw failure('snapshot', 'mismatch');
    phase = 'wrongpassword';
    const rejected = run(
      [helper, 'create', wrong],
      JSON.stringify({
        certificate,
        password: randomBytes(32).toString('hex'),
      }),
    );
    if (rejected.status === 0)
      throw failure('wrongpassword', 'unexpected-success', 0);
    // The helper exits 1 on rejection; a signal/spawn failure is not an expected rejection.
    if (rejected.status !== 1)
      throw failure(
        'wrongpassword',
        'command',
        rejected.status,
        helperDiagnostic(rejected.stderr),
      );
    if (existsSync(wrong)) throw failure('wrongpassword', 'owned-files');
    if (snapshot() !== baseline) throw failure('snapshot', 'mismatch');
  } catch (error) {
    original =
      typeof error === 'object' && error !== null && diagnostics.has(error)
        ? error
        : failure(phase, 'exception');
  } finally {
    const cleanup = (body: () => void): void => {
      try {
        at('cleanup', body);
      } catch (error) {
        cleanupErrors.push(safeSmokeDiagnostic(error));
      }
    };
    // Preserve unknown/failed helper directories, never recursively remove a live keychain.
    for (const directory of [owned, wrong])
      cleanup(() => {
        if (existsSync(directory))
          command('cleanup', [helper, 'remove', directory]);
      });
    cleanup(assertRestored);
    // Attempt every private-file removal even when helper recovery or another removal fails.
    for (const name of [
      'key.pem',
      'certificate.cnf',
      'cert.pem',
      'synthetic.p12',
      'harmless-mach-o',
    ])
      cleanup(() => rmSync(join(root, name), { force: true }));
    cleanup(() => {
      if (!existsSync(owned) && !existsSync(wrong)) rmdirSync(root);
    });
  }
  if (original || cleanupErrors.length) {
    const summary = [
      original ? safeSmokeDiagnostic(original) : undefined,
      ...cleanupErrors,
    ]
      .filter(Boolean)
      .join('; ');
    const error = new Error(summary);
    diagnostics.set(error, summary);
    throw error;
  }
}

if (import.meta.main) {
  try {
    const [helper, parent] = process.argv.slice(2);
    if (!helper || !parent || process.argv.length !== 4)
      throw new Error('Expected helper and private parent');
    smoke(
      helper,
      parent,
      {
        githubActions: process.env.GITHUB_ACTIONS,
        runnerEnvironment: process.env.RUNNER_ENVIRONMENT,
        platform: process.platform,
        uid: process.getuid!(),
      },
      (args, stdin) => {
        const result = Bun.spawnSync(args, {
          stdin:
            stdin === undefined ? undefined : new TextEncoder().encode(stdin),
          stdout: 'pipe',
          stderr: 'pipe',
          timeout: 60_000,
          env: {
            PATH: '/usr/bin:/bin:/usr/sbin:/sbin',
            HOME: process.env.HOME!,
            TMPDIR: process.env.TMPDIR!,
          },
        });
        return {
          status: result.exitCode,
          stdout: result.stdout.toString(),
          stderr: result.stderr.toString(),
        };
      },
    );
    console.log(
      'Disposable synthetic import/activate/sign/remove and wrong-password smoke passed; search/default restored; app never executed.',
    );
  } catch (error) {
    console.error(
      `Disposable synthetic keychain smoke failed: ${safeSmokeDiagnostic(error)}`,
    );
    process.exitCode = 1;
  }
}

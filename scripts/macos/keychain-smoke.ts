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
) => { status: number; stdout: string };

// Never include child output or arguments in failures: openssl/helper may handle private material.
export function smoke(
  helper: string,
  parent: string,
  context: SmokeContext,
  run: SmokeRunner,
): void {
  requireDisposable(context);
  if (
    realpathSync(parent) !== parent ||
    realpathSync(helper) !== helper ||
    !resolve(helper).startsWith('/')
  )
    throw new Error('Canonical paths required');
  const root = mkdtempSync(join(parent, 'tau-synthetic-'));
  chmodSync(root, 0o700);
  const owned = join(root, 'identity');
  const wrong = join(root, 'wrong-password');
  const command = (args: string[], stdin?: string): string => {
    const result = run(args, stdin);
    if (result.status !== 0) throw new Error('Synthetic smoke command failed');
    return result.stdout;
  };
  const snapshot = (): string =>
    JSON.stringify([
      command(['/usr/bin/security', 'list-keychains', '-d', 'user']),
      command(['/usr/bin/security', 'default-keychain', '-d', 'user']),
    ]);
  let baseline: string | undefined;
  const assertRestored = (): void => {
    if (baseline !== undefined && snapshot() !== baseline)
      throw new Error('Keychain globals not restored');
  };
  try {
    baseline = snapshot();
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
    command(
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
    const certificate = readFileSync(p12).toString('base64');
    const identity = JSON.parse(
      command(
        [helper, 'create', owned],
        JSON.stringify({ certificate, password }),
      ),
    ) as Record<string, string>;
    const fingerprint = command([
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
    if (
      Object.keys(identity).sort().join(',') !==
        'identity,keychain,name,teamId' ||
      identity.keychain !== join(owned, 'signing.keychain-db') ||
      !/^[A-F0-9]{40}$/.test(identity.identity ?? '') ||
      identity.identity !== fingerprint ||
      identity.name !== 'Tau Synthetic Smoke' ||
      identity.teamId !== 'SYNTHETIC1'
    )
      throw new Error('Synthetic identity mismatch');
    if (snapshot() !== baseline)
      throw new Error('Keychain globals changed after import');
    const binary = join(root, 'harmless-mach-o');
    copyFileSync('/usr/bin/true', binary);
    chmodSync(binary, 0o700);
    command([
      '/usr/bin/codesign',
      '--force',
      '--sign',
      identity.identity!,
      '--keychain',
      identity.keychain!,
      '--timestamp=none',
      binary,
    ]);
    command(['/usr/bin/codesign', '--verify', '--strict', binary]);
    command([helper, 'remove', owned]);
    if (snapshot() !== baseline)
      throw new Error('Keychain globals changed after remove');
    if (
      run(
        [helper, 'create', wrong],
        JSON.stringify({
          certificate,
          password: randomBytes(32).toString('hex'),
        }),
      ).status === 0
    )
      throw new Error('Wrong password unexpectedly accepted');
    if (existsSync(wrong)) throw new Error('Wrong password left owned files');
    if (snapshot() !== baseline)
      throw new Error('Keychain globals changed after rejected import');
  } finally {
    // Preserve unknown/failed helper directories, never recursively remove a live keychain.
    try {
      for (const directory of [owned, wrong]) {
        if (existsSync(directory)) command([helper, 'remove', directory]);
      }
      assertRestored();
    } finally {
      for (const name of [
        'key.pem',
        'certificate.cnf',
        'cert.pem',
        'synthetic.p12',
        'harmless-mach-o',
      ])
        rmSync(join(root, name), { force: true });
      if (!existsSync(owned) && !existsSync(wrong)) rmdirSync(root);
    }
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
        return { status: result.exitCode, stdout: result.stdout.toString() };
      },
    );
    console.log(
      'Disposable synthetic import/sign/remove and wrong-password smoke passed; app never executed.',
    );
  } catch {
    console.error(
      'Disposable synthetic keychain smoke failed; no credential diagnostics emitted.',
    );
    process.exitCode = 1;
  }
}

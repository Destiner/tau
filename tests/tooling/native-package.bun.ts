import { describe, expect, test } from 'bun:test';
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
  mkdirSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  validateInputs,
  verifyGitHub,
  type Api,
} from '../../scripts/macos/package-preflight';
import {
  requireDisposable,
  safeSmokeDiagnostic,
  smoke,
  type SmokeRunner,
} from '../../scripts/macos/keychain-smoke';

const sha = 'a'.repeat(40);
const cargo = '[package]\nname = "tau-gpui"\nversion = "0.3.0"\n';
const lock = '[[package]]\nname = "tau-gpui"\nversion = "0.3.0"\n';
const context = {
  githubActions: 'true',
  runnerEnvironment: 'github-hosted',
  platform: 'darwin',
  uid: 501,
};
function fixture(overrides: Record<string, unknown> = {}): Api {
  const data: Record<string, unknown> = {
    '/actions/workflows/ci.yml': { id: 7, path: '.github/workflows/ci.yml' },
    [`/actions/workflows/ci.yml/runs?event=push&branch=main&head_sha=${sha}&per_page=100`]:
      {
        workflow_runs: [
          {
            id: 9,
            workflow_id: 7,
            path: '.github/workflows/ci.yml',
            event: 'push',
            head_branch: 'main',
            head_sha: sha,
            status: 'completed',
            conclusion: 'success',
          },
        ],
      },
    '/actions/runs/9/jobs?filter=latest&per_page=100': {
      total_count: 1,
      jobs: [
        {
          name: 'check',
          head_sha: sha,
          status: 'completed',
          conclusion: 'success',
        },
      ],
    },
    '/git/matching-refs/tags/v0.3.0': [{ ref: 'refs/tags/v0.3.01' }],
    '/git/ref/tags/v0.3.0': null,
    ...overrides,
  };
  return async (path) => {
    if (!(path in data)) throw new Error('Unexpected API request');
    return data[path];
  };
}
// Files and process responses are synthetic; never execute the supplied executable names.
function diagnosticFixture(
  body: (helper: string, root: string, run: SmokeRunner) => void,
): void {
  const root = mkdtempSync(
    join(realpathSync(tmpdir()), 'tau-smoke-diagnostic-'),
  );
  const helper = join(root, 'fake-helper');
  writeFileSync(helper, 'fake');
  const run: SmokeRunner = (args) => {
    if (args.includes('req')) {
      writeFileSync(args[args.indexOf('-keyout') + 1]!, 'fake');
      writeFileSync(args[args.indexOf('-out') + 1]!, 'fake');
    }
    if (args.includes('pkcs12'))
      writeFileSync(args[args.indexOf('-out') + 1]!, 'p12');
    if (args.includes('x509'))
      return { status: 0, stdout: `sha1 Fingerprint=${'A'.repeat(40)}` };
    if (args[0] === helper) {
      if (args[1] === 'remove') rmSync(args[2]!, { recursive: true });
      else {
        if (args[2]!.endsWith('wrong-password'))
          return { status: 1, stdout: '' };
        mkdirSync(args[2]!);
        return {
          status: 0,
          stdout: JSON.stringify({
            keychain: join(args[2]!, 'signing.keychain-db'),
            identity: 'A'.repeat(40),
            name: 'Tau Synthetic Smoke',
            teamId: 'SYNTHETIC1',
          }),
        };
      }
    }
    return { status: 0, stdout: 'stable' };
  };
  try {
    body(helper, root, run);
  } finally {
    rmSync(root, { recursive: true });
  }
}
function diagnostic(body: () => void): string {
  try {
    body();
  } catch (error) {
    const summary = safeSmokeDiagnostic(error);
    expect((error as Error).message).toBe(summary);
    return summary;
  }
  throw new Error('Expected synthetic smoke to fail');
}

describe('bounded synthetic smoke diagnostics (fake processes only)', () => {
  test('phase and numeric status for every command boundary', () => {
    const cases: [string, (args: string[], helper: string) => boolean][] = [
      ['snapshot', (a): boolean => a.includes('default-keychain')],
      ['createcertificate', (a): boolean => a.includes('req')],
      ['exportP12', (a): boolean => a.includes('pkcs12')],
      ['helpercreate', (a, h): boolean => a[0] === h && a[1] === 'create'],
      ['fingerprint', (a): boolean => a.includes('x509')],
      ['sign', (a): boolean => a.includes('--sign')],
      ['verify', (a): boolean => a.includes('--verify')],
      ['remove', (a, h): boolean => a[0] === h && a[1] === 'remove'],
      [
        'wrongpassword',
        (a): boolean => a[2]?.endsWith('wrong-password') ?? false,
      ],
    ];
    for (const [phase, matches] of cases)
      diagnosticFixture((helper, root, run) => {
        const summary = diagnostic(() =>
          smoke(helper, root, context, (args, stdin) =>
            matches(args, helper)
              ? { status: 37, stdout: 'SECRET', stderr: 'SECRET' }
              : run(args, stdin),
          ),
        );
        expect(summary).toContain(
          `phase=${phase} category=command exitstatus=37`,
        );
        expect(summary).not.toContain('SECRET');
      });
  });
  test('context/setup, malformed metadata and thrown runner/filesystem errors are sanitized', () => {
    diagnosticFixture((helper, root, run) => {
      expect(
        diagnostic(() => smoke(helper, root, { ...context, uid: 0 }, run)),
      ).toContain('phase=context');
      expect(
        diagnostic(() => smoke(`${helper}/SECRET`, root, context, run)),
      ).toContain('phase=setup');
      expect(
        diagnostic(() =>
          smoke(helper, root, context, (args, stdin) => {
            if (args[0] === helper && args[1] === 'create')
              return { status: 0, stdout: 'SECRET invalid JSON' };
            return run(args, stdin);
          }),
        ),
      ).toContain('phase=metadata category=exception');
      const summary = diagnostic(() =>
        smoke(helper, root, context, (args, stdin) => {
          if (args.includes('req'))
            throw new Error('SECRET runner argv stdin stderr');
          return run(args, stdin);
        }),
      );
      expect(summary).toBe(
        'phase=createcertificate category=exception exitstatus=unavailable',
      );
      // Fake command claims success without producing the private key: chmod must not leak its path.
      expect(
        diagnostic(() =>
          smoke(helper, root, context, (args, stdin) =>
            args.includes('req') ? { status: 0, stdout: '' } : run(args, stdin),
          ),
        ),
      ).toBe(
        'phase=createcertificate category=exception exitstatus=unavailable',
      );
    });
    expect(safeSmokeDiagnostic(new Error('SECRET'))).not.toContain('SECRET');
  });
  test('only exact known helper messages and bounded Security statuses survive', () => {
    for (const message of [
      'Invalid ownership marker',
      'Writable directory ancestor',
      'Expected exactly one partition ACL',
      'Required Security access API unavailable',
      'Creation failed; owned cleanup or search-list restoration incomplete',
      'Import PKCS12 failed (OSStatus -25293)',
      'Save key access failed (OSStatus -2147483648)',
      'Restore search list failed (OSStatus 2147483647)',
    ])
      diagnosticFixture((helper, root, run) => {
        const summary = diagnostic(() =>
          smoke(helper, root, context, (args, stdin) =>
            args[0] === helper
              ? { status: 1, stdout: 'SECRET', stderr: `${message}\n` }
              : run(args, stdin),
          ),
        );
        expect(summary).toContain(`helper=${message}`);
        expect(summary).not.toContain('SECRET');
      });
    for (const stderr of [
      'SECRET',
      'Invalid ownership marker SECRET',
      'Invalid ownership marker\nSECRET',
      'Invalid ownership marker\n\n',
      'Invalid ownership marker\r\n',
      'SECRET failed (OSStatus -25293)',
      'Import PKCS12 failed (OSStatus -2147483649)',
      'Import PKCS12 failed (OSStatus 2147483648)',
      'Import PKCS12 failed (OSStatus +1)',
      'Import PKCS12 failed (OSStatus 01)',
      'Import PKCS12 failed (OSStatus -0)',
      'Import PKCS12 failed (OSStatus -25293)\n\n',
      'Import PKCS12 failed (OSStatus -25293)\r\n',
      'Import PKCS12 failed (OSStatus 1) SECRET',
      'SECRET'.repeat(100),
    ])
      diagnosticFixture((helper, root, run) => {
        expect(
          diagnostic(() =>
            smoke(helper, root, context, (args, stdin) =>
              args[0] === helper
                ? { status: 1, stdout: 'SECRET', stderr }
                : run(args, stdin),
            ),
          ),
        ).toBe('phase=helpercreate category=command exitstatus=1');
      });
    diagnosticFixture((helper, root, run) => {
      expect(
        diagnostic(() =>
          smoke(helper, root, context, (args, stdin) =>
            args.includes('req')
              ? {
                  status: 2,
                  stdout: 'SECRET',
                  stderr: 'Invalid ownership marker',
                }
              : run(args, stdin),
          ),
        ),
      ).toBe('phase=createcertificate category=command exitstatus=2');
    });
  });
  test('original plus cleanup failure survive; owned partial directory preserved and crypto removed', () => {
    diagnosticFixture((helper, root, run) => {
      let owned = '';
      const summary = diagnostic(() =>
        smoke(helper, root, context, (args, stdin) => {
          if (args[0] === helper) {
            owned = args[2]!;
            if (args[1] === 'create') {
              mkdirSync(owned);
              return {
                status: 4,
                stdout: 'SECRET',
                stderr: 'Invalid ownership marker',
              };
            }
            return {
              status: 5,
              stdout: '',
              stderr: 'Delete owned keychain failed (OSStatus -50)',
            };
          }
          return run(args, stdin);
        }),
      );
      expect(summary).toBe(
        'phase=helpercreate category=command exitstatus=4 helper=Invalid ownership marker; phase=cleanup category=command exitstatus=5 helper=Delete owned keychain failed (OSStatus -50)',
      );
      expect(existsSync(owned)).toBe(true);
      for (const name of [
        'key.pem',
        'cert.pem',
        'synthetic.p12',
        'certificate.cnf',
      ])
        expect(existsSync(join(owned, '..', name))).toBe(false);
    });
  });
  test('thrown cleanup errors cannot replace the original or leak raw errors', () => {
    diagnosticFixture((helper, root, run) => {
      const summary = diagnostic(() =>
        smoke(helper, root, context, (args, stdin) => {
          if (args[0] === helper && args[1] === 'remove')
            throw new Error('SECRET cleanup');
          if (args.includes('--sign')) throw new Error('SECRET sign');
          return run(args, stdin);
        }),
      );
      expect(summary).toBe(
        'phase=sign category=exception exitstatus=unavailable; phase=cleanup category=exception exitstatus=unavailable',
      );
    });
  });
  test('wrong-password success fails rather than being treated as expected rejection', () => {
    diagnosticFixture((helper, root, run) => {
      expect(
        diagnostic(() =>
          smoke(helper, root, context, (args, stdin) =>
            args[2]?.endsWith('wrong-password')
              ? { status: 0, stdout: '' }
              : run(args, stdin),
          ),
        ),
      ).toBe('phase=wrongpassword category=unexpected-success exitstatus=0');
    });
  });
  test('missing default snapshot cannot count as restored', () => {
    diagnosticFixture((helper, root, run) => {
      expect(
        diagnostic(() =>
          smoke(helper, root, context, (args, stdin) =>
            args.includes('default-keychain')
              ? { status: 44, stdout: 'SECRET' }
              : run(args, stdin),
          ),
        ),
      ).toBe(
        'phase=snapshot category=command exitstatus=44; phase=cleanup category=missing-baseline exitstatus=unavailable',
      );
    });
  });
});

describe('native package fail-closed policy (no network/OS APIs)', () => {
  test('exact main source and all versions', () => {
    expect(() =>
      validateInputs(
        'refs/heads/main',
        sha,
        sha,
        '0.3.0',
        '0.3.0',
        cargo,
        lock,
      ),
    ).not.toThrow();
    for (const [ref, commit, version, root, native, locked] of [
      ['refs/heads/topic', sha, '0.3.0', '0.3.0', cargo, lock],
      ['refs/heads/main', sha.toUpperCase(), '0.3.0', '0.3.0', cargo, lock],
      ['refs/heads/main', 'b'.repeat(40), '0.3.0', '0.3.0', cargo, lock],
      ['refs/heads/main', sha, '0.3.0; echo bad', '0.3.0', cargo, lock],
      ['refs/heads/main', sha, '0.3.0', '0.2.1', cargo, lock],
      [
        'refs/heads/main',
        sha,
        '0.3.0',
        '0.3.0',
        cargo.replace('0.3.0', '0.2.1'),
        lock,
      ],
      [
        'refs/heads/main',
        sha,
        '0.3.0',
        '0.3.0',
        cargo,
        lock.replace('tau-gpui', 'other'),
      ],
    ])
      expect(() =>
        validateInputs(ref!, sha, commit!, version!, root!, native!, locked!),
      ).toThrow();
  });
  test('exact CI push workflow run, aggregate job and exact public tag absence', async () => {
    expect(await verifyGitHub(fixture(), sha, '0.3.0')).toBe(9);
    for (const override of [
      {
        '/actions/workflows/ci.yml': {
          id: 8,
          path: '.github/workflows/other.yml',
        },
      },
      {
        '/actions/runs/9/jobs?filter=latest&per_page=100': {
          total_count: 1,
          jobs: [
            {
              name: 'check',
              head_sha: sha,
              status: 'in_progress',
              conclusion: null,
            },
          ],
        },
      },
      { '/git/matching-refs/tags/v0.3.0': [{ ref: 'refs/tags/v0.3.0' }] },
      { '/git/ref/tags/v0.3.0': { ref: 'refs/tags/v0.3.0' } },
    ])
      await expect(
        verifyGitHub(fixture(override), sha, '0.3.0'),
      ).rejects.toThrow();
    const path = `/actions/workflows/ci.yml/runs?event=push&branch=main&head_sha=${sha}&per_page=100`;
    for (const change of [
      { workflow_id: 8 },
      { event: 'workflow_dispatch' },
      { head_sha: 'b'.repeat(40) },
      { head_branch: 'topic' },
      { status: 'in_progress' },
      { conclusion: 'failure' },
    ]) {
      const original = (await fixture()(path)) as {
        workflow_runs: Record<string, unknown>[];
      };
      await expect(
        verifyGitHub(
          fixture({
            [path]: {
              workflow_runs: [{ ...original.workflow_runs[0], ...change }],
            },
          }),
          sha,
          '0.3.0',
        ),
      ).rejects.toThrow();
    }
    await expect(
      verifyGitHub(
        async () => {
          throw new Error('API unavailable');
        },
        sha,
        '0.3.0',
      ),
    ).rejects.toThrow();
  });
  test('smoke refuses local/self-hosted/root before commands', () => {
    for (const change of [
      { githubActions: undefined },
      { runnerEnvironment: 'self-hosted' },
      { platform: 'linux' },
      { uid: 0 },
    ])
      expect(() => requireDisposable({ ...context, ...change })).toThrow();
  });
  test('smoke fake import/sign/remove/wrong password, private stdin and owned cleanup', () => {
    const root = mkdtempSync(join(realpathSync(tmpdir()), 'tau-smoke-fake-'));
    const helper = join(root, 'fake-helper');
    writeFileSync(helper, 'fake');
    const calls: string[][] = [];
    let password = '';
    try {
      smoke(helper, root, context, (args, stdin) => {
        calls.push(args);
        if (args[0] === helper) {
          const directory = args[2]!;
          if (args[1] === 'remove') {
            rmSync(directory, { recursive: true });
            return { status: 0, stdout: '' };
          }
          const credential = JSON.parse(stdin!);
          expect(credential.certificate).toBe('cDEy');
          if (credential.password !== password)
            return { status: 1, stdout: '' };
          mkdirSync(directory);
          return {
            status: 0,
            stdout: JSON.stringify({
              keychain: join(directory, 'signing.keychain-db'),
              identity: 'A'.repeat(40),
              name: 'Tau Synthetic Smoke',
              teamId: 'SYNTHETIC1',
            }),
          };
        }
        if (args.includes('req')) {
          password = stdin!.trim();
          writeFileSync(
            args[args.indexOf('-keyout') + 1]!,
            'fake-encrypted-key',
          );
          writeFileSync(args[args.indexOf('-out') + 1]!, 'fake-cert');
        }
        if (args.includes('pkcs12')) {
          expect(stdin).toBe(`${password}\n${password}\n`);
          writeFileSync(args[args.indexOf('-out') + 1]!, 'p12');
        }
        if (args.includes('x509'))
          return {
            status: 0,
            stdout: `sha1 Fingerprint=${'AA:'.repeat(19)}AA\n`,
          };
        return { status: 0, stdout: args.includes('security') ? 'stable' : '' };
      });
      expect(
        calls.filter((args) => args[0] === helper).map((args) => args[1]),
      ).toEqual(['create', 'remove', 'create']);
      expect(JSON.stringify(calls)).not.toContain(password);
      expect(existsSync(helper)).toBe(true);
    } finally {
      rmSync(root, { recursive: true });
    }
  });
  test('smoke surfaces helper/identity/signing failures and removes partial owned directories', () => {
    for (const failure of ['import', 'identity', 'sign']) {
      const root = mkdtempSync(
        join(realpathSync(tmpdir()), 'tau-smoke-reject-'),
      );
      const helper = join(root, 'fake-helper');
      writeFileSync(helper, 'fake');
      let removed = false;
      try {
        expect(() =>
          smoke(helper, root, context, (args) => {
            if (args.includes('req')) {
              writeFileSync(args[args.indexOf('-keyout') + 1]!, 'fake');
              writeFileSync(args[args.indexOf('-out') + 1]!, 'fake');
            }
            if (args.includes('pkcs12'))
              writeFileSync(args[args.indexOf('-out') + 1]!, 'p12');
            if (args.includes('x509'))
              return {
                status: 0,
                stdout: `sha1 Fingerprint=${'A'.repeat(40)}`,
              };
            if (args[0] === helper && args[1] === 'create') {
              mkdirSync(args[2]!);
              return {
                status: failure === 'import' ? 1 : 0,
                stdout: JSON.stringify({
                  keychain: join(args[2]!, 'signing.keychain-db'),
                  identity: 'A'.repeat(40),
                  name:
                    failure === 'identity'
                      ? 'Unexpected'
                      : 'Tau Synthetic Smoke',
                  teamId: 'SYNTHETIC1',
                }),
              };
            }
            if (args[0] === helper && args[1] === 'remove') {
              rmSync(args[2]!, { recursive: true });
              removed = true;
            }
            if (args.includes('--sign')) return { status: 1, stdout: '' };
            return { status: 0, stdout: '' };
          }),
        ).toThrow();
        expect(removed).toBe(true);
        expect(readFileSync(helper, 'utf8')).toBe('fake');
      } finally {
        rmSync(root, { recursive: true });
      }
    }
  });
  test('manual synthetic diagnostic cannot replace push CI or use production credentials', () => {
    const workflow = readFileSync(
      new URL('../../.github/workflows/ci.yml', import.meta.url),
      'utf8',
    );
    expect(workflow).toContain("inputs.keychain_smoke && 'keychain-smoke-'");
    expect(workflow).toContain('default: false');
    expect(workflow).toContain('if: always() && !inputs.keychain_smoke');
    expect(
      workflow.match(/if: \$\{\{ !inputs.keychain_smoke \}\}/g),
    ).toHaveLength(2);
    const job = workflow
      .split('\n  keychain-smoke:\n')[1]
      ?.split('\n  check:')[0];
    expect(job).toBeDefined();
    expect(job).toContain(
      "if: github.event_name == 'workflow_dispatch' && inputs.keychain_smoke",
    );
    expect(job).toContain('bun scripts/macos/keychain-smoke.ts');
    expect(job).not.toMatch(
      /secrets\.|environment:|macos-release|upload-artifact|pull_request_target|permissions:/,
    );
    for (const use of job!.matchAll(/uses: ([^\s]+)/g))
      expect(use[1]).toMatch(/@[a-f0-9]{40}$/);
  });
  test('workflow privilege, transfer and credential boundaries', () => {
    const workflow = readFileSync(
      new URL('../../.github/workflows/native-package.yml', import.meta.url),
      'utf8',
    );
    expect(workflow).not.toMatch(
      /contents: write|actions: write|pull_request:|push:|secrets: inherit|TAURI|cancel-in-progress: true|gh release|gh api|rm -rf/,
    );
    expect(workflow).toContain('actions: read');
    expect(workflow).toContain('environment: release');
    expect(workflow.match(/secrets\./g)?.length).toBe(5);
    expect(workflow).toContain(
      'artifact-ids: ${{ needs.build.outputs.artifact_id }}',
    );
    expect(workflow).toContain(
      'EXPECTED_SHA256: ${{ needs.build.outputs.tar_sha256 }}',
    );
    expect(workflow.indexOf('keychain-smoke.ts')).toBeLessThan(
      workflow.indexOf('secrets.APPLE_CERTIFICATE'),
    );
    expect(workflow.indexOf('verify-input')).toBeLessThan(
      workflow.indexOf('secrets.APPLE_CERTIFICATE'),
    );
    for (const use of workflow.matchAll(/uses: ([^\s]+)/g))
      expect(use[1]).toMatch(/@[a-f0-9]{40}$/);
  });
});

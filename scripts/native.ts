import { spawnSync } from 'node:child_process';
import { isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const native = 'gpui/Cargo.toml';
const shared = [
  'crates/tau-core/Cargo.toml',
  'crates/tau-session/Cargo.toml',
  'crates/tau-observability/Cargo.toml',
];
const manifests = [...shared, native];

function cargo(command: string, manifest: string, ...args: string[]): string[] {
  return ['cargo', command, '--manifest-path', manifest, ...args];
}

function developmentArgs(args: string[]): string[] {
  const seen = new Set<string>();
  for (let i = 0; i < args.length; i += 2) {
    const flag = args[i]!;
    const value = args[i + 1];
    if (
      !['--isolated', '--project', '--session'].includes(flag) ||
      seen.has(flag) ||
      !value ||
      value.startsWith('--')
    )
      throw new Error(
        'Use --isolated <absolute directory>, optionally --project <absolute directory> and --session <id or path>.',
      );
    if (flag !== '--session' && !isAbsolute(value))
      throw new Error(`${flag} requires an absolute directory.`);
    seen.add(flag);
  }
  if (!seen.has('--isolated'))
    throw new Error(
      'Development requires --isolated <absolute directory>; it must not select production storage implicitly.',
    );
  return args;
}

export function commands(command: string, input: string[] = []): string[][] {
  const args = input[0] === '--' ? input.slice(1) : input;
  if (command !== 'dev' && args.length)
    throw new Error(
      `${command} does not accept extra arguments; use Cargo directly for focused checks.`,
    );
  switch (command) {
    case 'dev':
      return [
        cargo(
          'run',
          native,
          '--locked',
          '--bin',
          'tau',
          '--',
          ...developmentArgs(args),
        ),
      ];
    case 'build':
      return [cargo('build', native, '--locked', '--release', '--bin', 'tau')];
    case 'format':
      return manifests.map((manifest) => cargo('fmt', manifest));
    case 'check':
      return manifests.flatMap((manifest) => [
        cargo('fmt', manifest, '--check'),
        cargo(
          'clippy',
          manifest,
          '--locked',
          '--all-targets',
          '--all-features',
          '--',
          '-D',
          'warnings',
        ),
      ]);
    case 'test-core':
      return shared.map((manifest) =>
        cargo(
          'test',
          manifest,
          '--locked',
          '--all-features',
          '--',
          '--test-threads=1',
        ),
      );
    case 'test-native':
      return [
        cargo('test', native, '--locked', '--lib', '--', '--test-threads=1'),
        cargo(
          'test',
          native,
          '--locked',
          '--test',
          'input-semantics',
          '--test',
          'editor-values',
          '--test',
          'disabled-control-accessibility',
          '--',
          '--test-threads=1',
        ),
      ];
    case 'test-metal':
      return [
        cargo(
          'test',
          native,
          '--locked',
          '--test',
          'activity-paint',
          '--test',
          'diagram-paint',
          '--test',
          'composer-clipping',
          '--test',
          'composer-tail',
          '--test',
          'markdown-markers',
        ),
      ];
    default:
      throw new Error(`Unknown native command: ${command}`);
  }
}

if (import.meta.main) {
  try {
    const [command = '', ...args] = process.argv.slice(2);
    const plan = commands(command, args);
    const env = {
      ...process.env,
      CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS || '2',
      CARGO_TARGET_DIR:
        process.env.CARGO_TARGET_DIR || resolve(root, 'gpui/target'),
    };
    for (const [executable, ...arguments_] of plan) {
      const result = spawnSync(executable!, arguments_, {
        cwd: root,
        env,
        stdio: 'inherit',
      });
      if (result.error) throw result.error;
      if (result.status !== 0) process.exit(result.status ?? 1);
    }
  } catch (error) {
    console.error(
      error instanceof Error ? error.message : 'Native command failed.',
    );
    process.exitCode = 1;
  }
}

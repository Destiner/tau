import { expect, test } from 'bun:test';
import { commands } from '../../scripts/native';

test('development cannot silently start with production storage', () => {
  for (const args of [
    [],
    ['--project', '/private/tmp/project'],
    ['--isolated'],
    ['--isolated', 'relative'],
    ['--isolated', '/private/tmp/profile', '--isolated', '/private/tmp/other'],
    ['--isolated', '/private/tmp/profile', '--unknown'],
    ['--isolated', '--project'],
  ]) {
    expect(() => commands('dev', args)).toThrow();
  }
});

test('development passes exact explicit isolation and project arguments to the real app', () => {
  const args = [
    '--isolated',
    '/private/tmp/Tau profile',
    '--project',
    '/private/tmp/project',
    '--session',
    'session-id',
  ];
  expect(commands('dev', ['--', ...args])).toEqual([
    [
      'cargo',
      'run',
      '--manifest-path',
      'gpui/Cargo.toml',
      '--locked',
      '--bin',
      'tau',
      '--',
      ...args,
    ],
  ]);
});

test('normal build targets the optimized product without test features', () => {
  expect(commands('build')).toEqual([
    [
      'cargo',
      'build',
      '--manifest-path',
      'gpui/Cargo.toml',
      '--locked',
      '--release',
      '--bin',
      'tau',
    ],
  ]);
});

test('all shared crates and the app retain strict formatting and clippy gates', () => {
  const checks = commands('check');
  expect(checks).toHaveLength(8);
  expect(
    checks
      .filter((c) => c[1] === 'clippy')
      .every(
        (c) =>
          c.includes('--all-features') &&
          c.includes('--all-targets') &&
          c.slice(-3).join(' ') === '-- -D warnings',
      ),
  ).toBe(true);
  expect(
    checks.filter((c) => c[1] === 'fmt').every((c) => c.includes('--check')),
  ).toBe(true);
});

test('default test targets retain all paint and editor suites without live OS or Pi opt-ins', () => {
  const plans = ['test-core', 'test-native', 'test-metal'].flatMap((command) =>
    commands(command),
  );
  expect(plans).toHaveLength(6);
  for (const plan of plans) {
    expect(plan).toContain('--locked');
    expect(plan).not.toContain('--ignored');
    expect(plan).not.toContain('native-os-tests');
  }
  const targets = plans.flatMap((c) =>
    c.filter((_, i) => c[i - 1] === '--test'),
  );
  expect(targets).toEqual([
    'input-semantics',
    'editor-values',
    'disabled-control-accessibility',
    'activity-paint',
    'diagram-paint',
    'composer-clipping',
    'composer-tail',
    'markdown-markers',
  ]);
});

test('unknown commands or arbitrary cargo overrides fail before execution', () => {
  expect(() => commands('release')).toThrow();
  expect(() =>
    commands('test-native', ['--features', 'native-os-tests']),
  ).toThrow();
  expect(() => commands('build', ['--bin', 'prototype'])).toThrow();
});

import { expect, test } from 'bun:test';
import { mkdtempSync, mkdirSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  assertFresh,
  bundleId,
  plan,
  plist,
  verifyBinary,
} from '../scripts/package';

test('defaults build the real release binary, never a prototype or prepare', () => {
  const options = plan([]);
  expect(options.output).toEndWith('/gpui/target/bundle/macos/Tau Native.app');
  expect(options.build).toContain('--release');
  expect(
    options.build.slice(
      options.build.indexOf('--bin'),
      options.build.indexOf('--bin') + 2,
    ),
  ).toEqual(['--bin', 'tau']);
  expect(options.noBuild).toBe(false);
  expect(options.binary).toEndWith('/target/release/tau');
  expect(plan(['--debug']).build).not.toContain('--release');
  expect(plan(['--debug', '--no-build']).binary).toEndWith('/target/debug/tau');
  expect(plan(['--no-build']).noBuild).toBe(true);
});

test('output requires absolute distinct app name and rejects unknown options', () => {
  expect(plan(['--output', '/tmp/native-preview/Tau Native.app']).output).toBe(
    '/tmp/native-preview/Tau Native.app',
  );
  for (const args of [
    ['--output'],
    ['--output', 'Tau Native.app'],
    ['--output', '/Applications/Tau.app'],
    ['--prepare'],
  ]) {
    expect(() => plan(args)).toThrow();
  }
});

test('metadata has distinct identity, real executable and numeric version', () => {
  const xml = plist('0.1.0', true);
  expect(bundleId).not.toBe('com.destiner.tau');
  for (const value of [
    bundleId,
    '<string>tau</string>',
    '<string>0.1.0</string>',
    'GPUI debug preview',
    'icon.icns',
  ])
    expect(xml).toContain(value);
  expect(() => plist('<bad>', false)).toThrow();
});

test('fresh-only output preserves directories and dangling symlinks', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tau-package-test-'));
  try {
    const output = join(dir, 'Tau Native.app');
    assertFresh(output);
    mkdirSync(output);
    expect(() => assertFresh(output)).toThrow('already exists');
    rmSync(output, { recursive: true });
    symlinkSync(join(dir, 'missing'), output);
    expect(() => assertFresh(output)).toThrow('already exists');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('Mach-O, architecture, linkage and rpaths are inspected without a GUI', () => {
  const calls: string[][] = [];
  const responses = [
    'Mach-O 64-bit executable arm64',
    'arm64',
    'tau:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1)',
    'cmd LC_RPATH\n cmdsize 32\n path /usr/lib/ (offset 12)',
  ];
  const exec = (args: string[]): string => {
    calls.push(args);
    return responses[calls.length - 1]!;
  };
  expect(verifyBinary('/tmp/tau', exec)).toBe('arm64');
  expect(calls.map((args) => args.slice(0, 2))).toEqual([
    ['/usr/bin/file', '-b'],
    ['/usr/bin/lipo', '-archs'],
    ['/usr/bin/otool', '-L'],
    ['/usr/bin/otool', '-l'],
  ]);
  for (const [index, value] of [
    [0, 'ELF executable'],
    [1, 'i386'],
    [2, 'tau:\n\t/opt/homebrew/lib/libfoo.dylib (compatibility version 1)'],
    [3, 'cmd LC_RPATH\n cmdsize 32\n path /Users/test/build (offset 12)'],
  ] as const) {
    const changed = [...responses];
    changed[index] = value;
    let i = 0;
    expect(() => verifyBinary('/tmp/tau', () => changed[i++]!)).toThrow();
  }
});

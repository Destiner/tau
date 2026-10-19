import { expect, test } from 'bun:test';
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
} from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  assertFresh,
  bundleId,
  plan,
  plist,
  verifyBinary,
} from '../scripts/package';

test('defaults build the real release binary, never a prototype or prepare', () => {
  const options = plan([]);
  expect(options.output).toEndWith('/gpui/target/bundle/macos/Tau.app');
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

test('packaging owns native app icons and retains editable artwork', () => {
  const asset = (name: string): Buffer =>
    readFileSync(new URL(`../assets/app-icons/${name}`, import.meta.url));
  expect(asset('icon.icns').subarray(0, 4).toString()).toBe('icns');
  expect([...asset('icon.png').subarray(0, 8)]).toEqual([
    137, 80, 78, 71, 13, 10, 26, 10,
  ]);
  expect(asset('icon.svg').toString()).toContain('<svg');
  expect(
    readFileSync(new URL('../scripts/package.ts', import.meta.url), 'utf8'),
  ).toContain("join(root, 'assets/app-icons/icon.icns')");
});

test('output requires absolute app name and rejects unknown options', () => {
  expect(plan(['--output', '/tmp/native-preview/Tau.app']).output).toBe(
    '/tmp/native-preview/Tau.app',
  );
  for (const args of [
    ['--output'],
    ['--output', 'Tau.app'],
    ['--output', '/Applications/Tau.app'],
    ['--output', '/tmp/Tau Native.app'],
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
    '<key>CFBundleName</key><string>Tau</string>',
    '<key>CFBundleDisplayName</key><string>Tau</string>',
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
    const output = join(dir, 'Tau.app');
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

test('Applications destinations reject lexical and symlink aliases even when fresh', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tau-package-location-'));
  try {
    // Bun caches homedir at startup; the child owns a real Applications fixture on every host.
    const result = spawnSync(
      process.execPath,
      [fileURLToPath(new URL('./package-location.ts', import.meta.url)), dir],
      { env: { ...process.env, HOME: dir }, encoding: 'utf8', timeout: 10_000 },
    );
    if (result.error) throw result.error;
    expect(result.status, result.stderr).toBe(0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

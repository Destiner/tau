import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { plist } from '../scripts/package';

const read = (path: string): string =>
  readFileSync(new URL(path, import.meta.url), 'utf8');
const packageVersion = (toml: string): string | undefined =>
  toml
    .split('[package]')[1]
    ?.split(/\n\[/)[0]
    ?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

// Root product metadata owns the version; Cargo supplies runtime and bundle metadata.
test('native Cargo and its local lock entry match the production version', () => {
  const version = JSON.parse(read('../../package.json')).version;
  expect(version).toBe('0.2.1');
  expect(packageVersion(read('../Cargo.toml'))).toBe(version);
  const native = read('../Cargo.lock')
    .split('[[package]]')
    .find((entry) => /^name = "tau-gpui"$/m.test(entry));
  expect(native?.match(/^version = "([^"]+)"$/m)?.[1]).toBe(version);
  for (const debug of [false, true]) {
    const xml = plist(version, debug);
    for (const key of ['CFBundleShortVersionString', 'CFBundleVersion'])
      expect(xml).toContain(`<key>${key}</key><string>${version}</string>`);
  }
});

test('Cargo declares tau as the sole product binary and default run target', () => {
  const cargo = read('../Cargo.toml');
  expect(cargo).toMatch(/^name = "tau-gpui"$/m);
  expect(cargo).toMatch(/^default-run = "tau"$/m);
  expect(cargo).toMatch(/^autobins = false$/m);
  const bins = cargo.split('[[bin]]').slice(1);
  expect(bins).toHaveLength(1);
  expect(bins[0]?.split(/\n\[/)[0]).toContain('name = "tau"');
  expect(bins[0]?.split(/\n\[/)[0]).toContain('path = "src/bin/tau.rs"');
});

test('production UI and packager use native Cargo metadata, not a display override', () => {
  expect(read('../src/app.rs')).toMatch(
    /fn app_version\(\) -> String\s*\{\s*env!\("CARGO_PKG_VERSION"\)\.into\(\)\s*\}/,
  );
  const packaging = read('../scripts/package.ts');
  expect(packaging).toContain("readFileSync(join(root, 'Cargo.toml'), 'utf8')");
  expect(packaging).toContain('const metadata = plist(version, options.debug)');
});

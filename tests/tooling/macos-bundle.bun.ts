import { afterAll, expect, test } from 'bun:test';
import { createHash } from 'node:crypto';
import { dlopen, FFIType } from 'bun:ffi';
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { plist } from '../../gpui/scripts/package';
import type { BuildManifest } from '../../scripts/macos/bundle';

const temp = realpathSync(mkdtempSync(join(tmpdir(), 'tau-bundle-tests-')));
const fixture = join(temp, 'repo');
function put(path: string, content: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
}
for (const path of [
  'scripts/macos/bundle.ts',
  'scripts/macos/licenses.ts',
  'gpui/scripts/package.ts',
]) {
  mkdirSync(dirname(join(fixture, path)), { recursive: true });
  copyFileSync(join(import.meta.dir, '../..', path), join(fixture, path));
}
put(join(fixture, 'package.json'), '{"version":"0.3.0","type":"module"}');
put(join(fixture, 'gpui/Cargo.toml'), '[package]\nversion = "0.3.0"\n');
put(
  join(fixture, 'gpui/Cargo.lock'),
  '[[package]]\nname = "tau-gpui"\nversion = "0.3.0"\n',
);
for (const path of [
  'LICENSE',
  'gpui/assets/Inter-LICENSE.txt',
  'gpui/assets/product-fonts/OFL.txt',
  'gpui/assets/product-icons/LICENSE',
  'gpui/assets/product-syntax/LICENSE',
  'gpui/assets/product-syntax/SOURCE.md',
  'gpui/assets/product-syntax/two-face-0.4.5-ACKNOWLEDGEMENTS.md',
  'gpui/assets/product-syntax/two-face-0.4.5-LICENSE-APACHE',
  'gpui/assets/product-syntax/two-face-0.4.5-LICENSE-MIT',
  'gpui/src/components/diagram/NOTICES.md',
  'gpui/src/components/diagram/LICENSE-INVENTORY.md',
  'gpui/src/components/diagram/licenses/example/LICENSE',
  'gpui/patches/compactor.patch',
  'gpui/scripts/prepare.ts',
  'gpui/.cache/crates/merman-render/src/compactor.rs',
  'gpui/.cache/crates/merman-layout-elk/src/lib.rs',
  'gpui/assets/app-icons/icon.icns',
])
  put(join(fixture, path), `fixture ${path}`);
copyFileSync(join(import.meta.dir, '../../LICENSE'), join(fixture, 'LICENSE'));
const api = (await import(
  join(fixture, 'scripts/macos/bundle.ts')
)) as typeof import('../../scripts/macos/bundle');
const expected = { version: '0.3.0', commit: 'a'.repeat(40) };
let sequence = 0;
let dirty = false;
let head = expected.commit;
let architecture = 'arm64';
let host = 'aarch64-apple-darwin';
let linkage = '/usr/lib/libSystem.B.dylib';
let minimum = '15.0';
const commands: string[][] = [];
function exec(args: string[]): string {
  commands.push(args);
  if (args[0] === '/usr/bin/uname')
    return args[1] === '-s' ? 'Darwin' : 'arm64';
  if (args[0] === 'rustc') return `host: ${host}\n`;
  if (args[0] === 'git')
    return args.includes('HEAD') ? head : dirty ? '?? extra' : '';
  if (args[0] === 'cargo' && args[1] === 'metadata')
    return JSON.stringify({
      packages: [
        {
          id: 'tau',
          name: 'tau-gpui',
          version: expected.version,
          manifest_path: join(fixture, 'gpui/Cargo.toml'),
          source: null,
          license: 'MIT',
          license_file: null,
        },
      ],
      resolve: { root: 'tau', nodes: [{ id: 'tau', deps: [] }] },
    });
  if (args[0] === 'cargo') {
    const target = args[args.indexOf('--target-dir') + 1]!;
    put(join(target, 'release/tau'), 'fixture Mach-O');
    return '';
  }
  if (args[0] === '/usr/bin/file') return 'Mach-O 64-bit executable arm64';
  if (args[0] === '/usr/bin/lipo') return architecture;
  if (args[0] === '/usr/bin/otool')
    return args[1] === '-L'
      ? `tau:\n\t${linkage} (compatibility version 1.0.0)\n`
      : `Load command 1\n cmd LC_BUILD_VERSION\n cmdsize 32\n platform 1\n minos ${minimum}\n sdk 26.0\n`;
  if (args[0] === '/usr/bin/plutil') return 'OK';
  throw new Error(`Unexpected command: ${args.join(' ')}`);
}
function candidate(): { output: string; manifest: BuildManifest } {
  const output = join(temp, `candidate-${sequence++}`);
  return {
    output,
    manifest: api.prepareCandidate(
      { ...expected, output, targetDir: join(temp, 'target') },
      exec,
    ),
  };
}
function rewrite(output: string, manifest: BuildManifest): void {
  writeFileSync(join(output, 'build.json'), JSON.stringify(manifest));
}
afterAll((): void => {
  rmSync(temp, { recursive: true, force: true });
});

test('production metadata is distinct; preview API remains the default', (): void => {
  expect(plist('0.3.0', false)).toContain('com.destiner.tau.native-preview');
  expect(plist('0.3.0', false)).toContain('ad-hoc');
  expect(plist('0.3.0', false, 'production')).toContain(
    '<string>com.destiner.tau</string>',
  );
  expect(plist('0.3.0', false, 'production')).not.toContain('preview');
});

test('fresh candidate, normal host build, provenance, copied legal source and complete hashes', (): void => {
  const { output, manifest } = candidate();
  expect(api.verifyCandidate(output, expected, exec)).toEqual(manifest);
  expect(commands.find((c) => c[0] === 'cargo')).toEqual([
    'cargo',
    'build',
    '--locked',
    '--release',
    '--jobs',
    '2',
    '--manifest-path',
    join(fixture, 'gpui/Cargo.toml'),
    '--bin',
    'tau',
    '--target-dir',
    join(temp, 'target'),
  ]);
  expect(commands.some((c) => /codesign|prepare|install|ssh/.test(c[0]!))).toBe(
    false,
  );
  expect(
    existsSync(
      join(output, 'Tau.app/Contents/Resources/tau-native-preview.json'),
    ),
  ).toBe(false);
  const source = 'gpui/.cache/crates/merman-render/src/compactor.rs';
  const copied = join(output, 'Tau.app/Contents/Resources/Legal', source);
  expect(readFileSync(copied, 'utf8')).toBe(
    readFileSync(join(fixture, source), 'utf8'),
  );
  expect(lstatSync(copied).ino).not.toBe(lstatSync(join(fixture, source)).ino);
  const licenses = JSON.parse(
    readFileSync(
      join(
        output,
        'Tau.app/Contents/Resources/Legal/RustDependencies/index.json',
      ),
      'utf8',
    ),
  );
  expect(licenses.packages.map((p: { name: string }) => p.name)).toEqual([
    'tau-gpui',
  ]);
  expect(commands).toContainEqual([
    'cargo',
    'metadata',
    '--offline',
    '--locked',
    '--filter-platform',
    'aarch64-apple-darwin',
    '--format-version',
    '1',
    '--manifest-path',
    join(fixture, 'gpui/Cargo.toml'),
  ]);
  expect(
    manifest.files.every(
      (f) => f.path.startsWith('Tau.app/') && f.sha256.length === 64,
    ),
  ).toBe(true);
});

test('preflight refuses mismatched metadata, dirty trees, commit, and rustc host', (): void => {
  const options = { ...expected, output: join(temp, 'rejected') };
  expect(() =>
    api.prepareCandidate({ ...options, version: '0.2.1' }, exec),
  ).toThrow('version mismatch');
  head = 'b'.repeat(40);
  expect(() => api.prepareCandidate(options, exec)).toThrow('Commit mismatch');
  head = expected.commit;
  dirty = true;
  expect(() => api.prepareCandidate(options, exec)).toThrow('dirty');
  dirty = false;
  host = 'x86_64-apple-darwin';
  expect(() => api.prepareCandidate(options, exec)).toThrow('rustc host');
  host = 'aarch64-apple-darwin';
  expect(() =>
    api.verifyCandidate(
      candidate().output,
      { ...expected, commit: 'c'.repeat(40) },
      exec,
    ),
  ).toThrow('metadata mismatch');
});

test('unsafe, existing, symlink and Applications alias destinations are refused before build', (): void => {
  const { output } = candidate();
  const link = join(temp, 'output-link');
  symlinkSync(output, link);
  const alias = join(temp, 'applications');
  symlinkSync('/Applications', alias);
  for (const destination of [
    output,
    link,
    'relative',
    '/Applications/tau-candidate',
    join(alias, 'tau-candidate'),
  ]) {
    const count = commands.length;
    expect(() =>
      api.prepareCandidate({ ...expected, output: destination }, exec),
    ).toThrow();
    expect(commands.length).toBe(count);
  }
});

test('altered, missing, extra and symlink files fail verification', (): void => {
  for (const mutation of ['altered', 'missing', 'extra', 'symlink']) {
    const { output } = candidate();
    const binary = join(output, 'Tau.app/Contents/MacOS/tau');
    if (mutation === 'altered') writeFileSync(binary, 'tampered');
    if (mutation === 'missing') rmSync(binary);
    if (mutation === 'extra') put(join(output, 'Tau.app/extra'), 'unexpected');
    if (mutation === 'symlink') {
      rmSync(binary);
      symlinkSync(join(temp, 'target/release/tau'), binary);
    }
    expect(() => api.verifyCandidate(output, expected, exec)).toThrow();
  }
});

test('malicious manifest schema, traversal, duplicate, hash, size and unexpected top-level contents fail closed', (): void => {
  for (const mutation of [
    'schema',
    'traversal',
    'duplicate',
    'hash',
    'size',
    'extra',
    'null',
  ]) {
    const { output, manifest } = candidate();
    if (mutation === 'schema') Reflect.set(manifest, 'schemaVersion', 2);
    if (mutation === 'traversal')
      manifest.files[0]!.path = 'Tau.app/../../outside';
    if (mutation === 'duplicate') manifest.files.push(manifest.files[0]!);
    if (mutation === 'hash') manifest.files[0]!.sha256 = 'b'.repeat(64);
    if (mutation === 'size') manifest.files[0]!.size = -1;
    if (mutation === 'extra') put(join(output, 'extra'), 'unexpected');
    rewrite(output, manifest);
    if (mutation === 'null') writeFileSync(join(output, 'build.json'), 'null');
    expect(() => api.verifyCandidate(output, expected, exec)).toThrow();
  }
});

test('plist/provenance are checked even if an attacker recomputes file hashes', (): void => {
  for (const path of [
    'Contents/Info.plist',
    'Contents/Resources/tau-build.json',
  ]) {
    const { output, manifest } = candidate();
    const entry = manifest.files.find(
      (file) => file.path === `Tau.app/${path}`,
    )!;
    const content = readFileSync(join(output, entry.path), 'utf8').replace(
      '0.3.0',
      '0.4.0',
    );
    writeFileSync(join(output, entry.path), content);
    entry.sha256 = createHash('sha256').update(content).digest('hex');
    entry.size = Buffer.byteLength(content);
    rewrite(output, manifest);
    expect(() => api.verifyCandidate(output, expected, exec)).toThrow(
      'Plist/provenance',
    );
  }
});

test('source symlinks fail closed and never publish a partial candidate', (): void => {
  const source = join(
    fixture,
    'gpui/.cache/crates/merman-render/src/compactor.rs',
  );
  const original = readFileSync(source);
  rmSync(source);
  symlinkSync(join(temp, 'target/release/tau'), source);
  const output = join(temp, 'bad-source');
  try {
    expect(() => api.prepareCandidate({ ...expected, output }, exec)).toThrow(
      'symlinks',
    );
    expect(existsSync(output)).toBe(false);
  } finally {
    rmSync(source);
    writeFileSync(source, original);
  }
});

test('candidate deployment is inspected even with recomputed binary inventory', (): void => {
  const { output, manifest } = candidate();
  const entry = manifest.files.find((file) =>
    file.path.endsWith('/MacOS/tau'),
  )!;
  const content = 'synthetic incompatible Mach-O';
  put(join(output, entry.path), content);
  entry.sha256 = createHash('sha256').update(content).digest('hex');
  entry.size = Buffer.byteLength(content);
  rewrite(output, manifest);
  minimum = '15.1';
  try {
    expect(() => api.verifyCandidate(output, expected, exec)).toThrow(
      'deployment target',
    );
  } finally {
    minimum = '15.0';
  }
});

test('exclusive publisher passes owned NUL-terminated Buffers through the FFI boundary', () => {
  for (const status of [0, -1]) {
    let closed = false;
    const open = ((
      _path: string,
      symbols: { renamex_np: { args: number[] } },
    ) => {
      expect(symbols.renamex_np.args).toEqual([
        FFIType.ptr,
        FFIType.ptr,
        FFIType.u32,
      ]);
      return {
        symbols: {
          renamex_np: (
            stage: Buffer,
            output: Buffer,
            flags: number,
          ): number => {
            Bun.gc(true);
            expect(Buffer.isBuffer(stage)).toBe(true);
            expect(Buffer.isBuffer(output)).toBe(true);
            expect(stage.toString()).toBe('/tmp/長い-stage\0');
            expect(output.toString()).toBe('/tmp/長い-output\0');
            expect(flags).toBe(4);
            return status;
          },
        },
        close: (): void => {
          closed = true;
        },
      };
    }) as unknown as typeof dlopen;
    const publish = (): void =>
      api.publishExclusive('/tmp/長い-stage', '/tmp/長い-output', open);
    if (status === 0) publish();
    else expect(publish).toThrow('exclusive');
    expect(closed).toBe(true);
  }
});

test('concurrent exclusive publication to fresh Unicode output has exactly one complete winner', async () => {
  const directory = join(temp, 'publication-' + '長い'.repeat(30));
  mkdirSync(directory);
  const output = join(directory, 'output-τ');
  const gate = join(directory, 'go');
  const children = ['one', 'two'].map((name) => {
    const stage = join(directory, name);
    put(join(stage, 'payload'), name);
    return Bun.spawn(
      [
        process.execPath,
        '--eval',
        `
      import { existsSync, writeFileSync } from 'node:fs';
      import { publishExclusive } from ${JSON.stringify(join(fixture, 'scripts/macos/bundle.ts'))};
      writeFileSync(${JSON.stringify(join(directory, name + '-ready'))}, 'ready');
      while (!existsSync(${JSON.stringify(gate)})) await Bun.sleep(5);
      try { publishExclusive(${JSON.stringify(stage)}, ${JSON.stringify(output)}); }
      catch { process.exit(1); }
    `,
      ],
      { env: {}, stdout: 'pipe', stderr: 'pipe' },
    );
  });
  try {
    const deadline = Date.now() + 10_000;
    while (
      !['one', 'two'].every((name) =>
        existsSync(join(directory, name + '-ready')),
      )
    ) {
      if (Date.now() > deadline)
        throw new Error('Publication workers did not reach barrier');
      await Bun.sleep(5);
    }
    expect(existsSync(output)).toBe(false);
    put(gate, 'go');
    expect(
      (await Promise.all(children.map((child) => child.exited))).sort(),
    ).toEqual([0, 1]);
    const winner = readFileSync(join(output, 'payload'), 'utf8');
    const loser = winner === 'one' ? 'two' : 'one';
    expect(readFileSync(join(directory, loser, 'payload'), 'utf8')).toBe(loser);
    expect(existsSync(join(directory, winner))).toBe(false);
    expect(() =>
      api.publishExclusive(join(directory, loser), output),
    ).toThrow();
    expect(readFileSync(join(output, 'payload'), 'utf8')).toBe(winner);
  } finally {
    for (const child of children) child.kill();
    await Promise.all(children.map((child) => child.exited));
  }
});

test('architecture and linkage are re-inspected, never inferred from manifest', (): void => {
  const { output } = candidate();
  architecture = 'x86_64';
  expect(() => api.verifyCandidate(output, expected, exec)).toThrow(
    'arm64 only',
  );
  architecture = 'arm64';
  linkage = '/opt/homebrew/lib/example.dylib';
  expect(() => api.verifyCandidate(output, expected, exec)).toThrow(
    'Non-system',
  );
  linkage = '/usr/lib/libSystem.B.dylib';
});

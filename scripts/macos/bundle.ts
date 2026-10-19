import { createHash } from 'node:crypto';
import {
  chmodSync,
  copyFileSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { dlopen, FFIType } from 'bun:ffi';
import { collectDependencyLicenses } from './licenses';
import {
  assertFresh,
  plist,
  verifyBinary,
  type Runner,
} from '../../gpui/scripts/package';

const root = fileURLToPath(new URL('../../', import.meta.url));
const identity = {
  bundleId: 'com.destiner.tau',
  architecture: 'arm64',
  minimumSystemVersion: '15.0',
} as const;
export interface BuildManifest {
  schemaVersion: 1;
  version: string;
  commit: string;
  bundleId: 'com.destiner.tau';
  architecture: 'arm64';
  minimumSystemVersion: '15.0';
  files: Array<{ path: string; sha256: string; size: number }>;
}
type Expected = { version: string; commit: string };

function validateExpected(expected: Expected): void {
  if (
    !/^\d+\.\d+\.\d+$/.test(expected.version) ||
    !/^[0-9a-f]{40}$/.test(expected.commit)
  )
    throw new Error('Invalid version or commit');
}

function regular(path: string): void {
  if (!lstatSync(path).isFile())
    throw new Error(`Expected regular file: ${path}`);
}

function files(directory: string, prefix = ''): BuildManifest['files'] {
  if (!lstatSync(directory).isDirectory())
    throw new Error('Expected regular directory');
  return readdirSync(directory)
    .sort()
    .flatMap((name): BuildManifest['files'] => {
      const path = join(directory, name);
      const entry = prefix ? `${prefix}/${name}` : name;
      const stat = lstatSync(path);
      if (stat.isSymbolicLink()) throw new Error('Symlinks forbidden');
      if (stat.isDirectory()) return files(path, entry);
      regular(path);
      return [
        {
          path: entry,
          sha256: createHash('sha256').update(readFileSync(path)).digest('hex'),
          size: stat.size,
        },
      ];
    });
}

function copy(source: string, destination: string): void {
  const stat = lstatSync(source);
  if (stat.isSymbolicLink()) throw new Error('Source symlinks forbidden');
  if (stat.isDirectory()) {
    mkdirSync(destination, { recursive: true });
    for (const name of readdirSync(source))
      copy(join(source, name), join(destination, name));
  } else {
    regular(source);
    mkdirSync(dirname(destination), { recursive: true });
    copyFileSync(source, destination);
  }
}

function provenance(expected: Expected): object {
  return {
    version: expected.version,
    commit: expected.commit,
    bundleID: identity.bundleId,
    target: 'aarch64-apple-darwin',
    minimumOS: identity.minimumSystemVersion,
  };
}

function preflight(expected: Expected, exec: Runner): void {
  validateExpected(expected);
  if (
    exec(['/usr/bin/uname', '-s']).trim() !== 'Darwin' ||
    exec(['/usr/bin/uname', '-m']).trim() !== 'arm64' ||
    !/^host: aarch64-apple-darwin$/m.test(exec(['rustc', '-vV']))
  )
    throw new Error(
      'Requires Darwin/arm64 and aarch64-apple-darwin rustc host',
    );
  if (
    exec(['git', '-C', root, 'rev-parse', 'HEAD']).trim() !== expected.commit ||
    exec([
      'git',
      '-C',
      root,
      'status',
      '--porcelain',
      '--untracked-files=all',
    ]).trim()
  )
    throw new Error('Commit mismatch or dirty worktree');
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
    version?: unknown;
  };
  const cargo = Bun.TOML.parse(
    readFileSync(join(root, 'gpui/Cargo.toml'), 'utf8'),
  ) as { package?: { version?: unknown } };
  const lock = Bun.TOML.parse(
    readFileSync(join(root, 'gpui/Cargo.lock'), 'utf8'),
  ) as { package?: Array<{ name?: string; version?: unknown }> };
  const local = lock.package?.filter((p) => p.name === 'tau-gpui');
  if (
    pkg.version !== expected.version ||
    cargo.package?.version !== expected.version ||
    local?.length !== 1 ||
    local[0]?.version !== expected.version
  )
    throw new Error('Package/Cargo/lock version mismatch');
}

function legal(resources: string): void {
  const assets = [
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
    'gpui/src/components/diagram/licenses',
    'gpui/patches',
    'gpui/Cargo.toml',
    'gpui/Cargo.lock',
    'gpui/scripts/prepare.ts',
    'gpui/.cache/crates/merman-render',
    'gpui/.cache/crates/merman-layout-elk',
  ];
  for (const asset of assets) {
    let ancestor = root;
    for (const component of asset.split('/')) {
      ancestor = join(ancestor, component);
      if (lstatSync(ancestor).isSymbolicLink())
        throw new Error('Legal asset ancestor symlinks forbidden');
    }
    copy(join(root, asset), join(resources, 'Legal', asset));
  }
  writeFileSync(
    join(resources, 'Legal/SOURCE-OFFER.txt'),
    `Corresponding source accompanies this binary in Legal/gpui/.cache/crates and Legal/gpui/patches.\nOriginal release sources and checksums, including unmodified EPL-2.0 merman-elk-layered 0.8.0 and MPL dependencies, are identified by immutable versioned crates.io links in Legal/gpui/src/components/diagram/{NOTICES,LICENSE-INVENTORY}.md.\nApply the included patches to the exact upstream releases as described by Legal/gpui/scripts/prepare.ts. Preserve all separate EPL, MPL, font and upstream notices.\n`,
  );
}

// Darwin RENAME_EXCL publishes the complete directory atomically without replacing even an empty destination.
export function publishExclusive(
  stage: string,
  output: string,
  open: typeof dlopen = dlopen,
): void {
  const libc = open('/usr/lib/libSystem.B.dylib', {
    renamex_np: {
      args: [FFIType.ptr, FFIType.ptr, FFIType.u32],
      returns: FFIType.i32,
    },
  });
  try {
    if (
      // Passing Buffers keeps their backing storage alive through the native call.
      libc.symbols.renamex_np(
        Buffer.from(`${stage}\0`),
        Buffer.from(`${output}\0`),
        4,
      ) !== 0
    )
      throw new Error('Atomic exclusive publication failed');
  } finally {
    libc.close();
  }
}

export function prepareCandidate(
  options: {
    output: string;
    version: string;
    commit: string;
    targetDir?: string;
  },
  exec: (args: string[]) => string,
): BuildManifest {
  if (
    !isAbsolute(options.output) ||
    (options.targetDir !== undefined && !isAbsolute(options.targetDir))
  )
    throw new Error('Absolute output/targetDir required');
  const output = resolve(options.output);
  assertFresh(output);
  preflight(options, exec);
  const target = options.targetDir ?? join(root, 'gpui/target');
  exec([
    'cargo',
    'build',
    '--locked',
    '--release',
    '--jobs',
    '2',
    '--manifest-path',
    join(root, 'gpui/Cargo.toml'),
    '--bin',
    'tau',
    '--target-dir',
    target,
  ]);
  const binary = join(target, 'release/tau');
  regular(binary);
  if (verifyBinary(binary, exec) !== 'arm64')
    throw new Error('Candidate requires arm64 only');
  mkdirSync(dirname(output), { recursive: true });
  const stage = mkdtempSync(join(dirname(output), '.tau-candidate-'));
  try {
    const app = join(stage, 'Tau.app');
    const resources = join(app, 'Contents/Resources');
    mkdirSync(resources, { recursive: true });
    copy(binary, join(app, 'Contents/MacOS/tau'));
    chmodSync(join(app, 'Contents/MacOS/tau'), 0o755);
    copy(
      join(root, 'gpui/assets/app-icons/icon.icns'),
      join(resources, 'icon.icns'),
    );
    writeFileSync(
      join(app, 'Contents/Info.plist'),
      plist(options.version, false, 'production'),
    );
    writeFileSync(
      join(resources, 'tau-build.json'),
      JSON.stringify(provenance(options), null, 2) + '\n',
    );
    legal(resources);
    collectDependencyLicenses(
      JSON.parse(
        exec([
          'cargo',
          'metadata',
          '--offline',
          '--locked',
          '--filter-platform',
          'aarch64-apple-darwin',
          '--format-version',
          '1',
          '--manifest-path',
          join(root, 'gpui/Cargo.toml'),
        ]),
      ),
      join(resources, 'Legal'),
      root,
    );
    const manifest: BuildManifest = {
      schemaVersion: 1,
      version: options.version,
      commit: options.commit,
      ...identity,
      files: files(app, 'Tau.app'),
    };
    writeFileSync(
      join(stage, 'build.json'),
      JSON.stringify(manifest, null, 2) + '\n',
    );
    verifyCandidate(stage, options, exec);
    assertFresh(output);
    publishExclusive(stage, output);
    return manifest;
  } finally {
    rmSync(stage, { recursive: true, force: true });
  }
}

export function verifyCandidate(
  directory: string,
  expected: { version: string; commit: string },
  exec: (args: string[]) => string,
): BuildManifest {
  validateExpected(expected);
  if (!isAbsolute(directory) || !lstatSync(directory).isDirectory())
    throw new Error('Absolute regular candidate directory required');
  const top = readdirSync(directory).sort();
  if (JSON.stringify(top) !== JSON.stringify(['Tau.app', 'build.json']))
    throw new Error('Unexpected candidate contents');
  regular(join(directory, 'build.json'));
  const value: unknown = JSON.parse(
    readFileSync(join(directory, 'build.json'), 'utf8'),
  );
  if (!value || typeof value !== 'object') throw new Error('Invalid manifest');
  const manifest = value as BuildManifest;
  if (
    manifest.schemaVersion !== 1 ||
    manifest.version !== expected.version ||
    manifest.commit !== expected.commit ||
    Object.entries(identity).some(
      ([key, val]) => Reflect.get(manifest, key) !== val,
    ) ||
    !Array.isArray(manifest.files) ||
    !manifest.files.length
  )
    throw new Error('Manifest metadata mismatch');
  const paths = new Set<string>();
  for (const file of manifest.files) {
    if (
      !file ||
      typeof file.path !== 'string' ||
      !/^Tau\.app\/(?:[^/\\]+\/)*[^/\\]+$/.test(file.path) ||
      [...file.path].some((character) => character.charCodeAt(0) < 32) ||
      file.path.split('/').some((p) => p === '.' || p === '..') ||
      paths.has(file.path) ||
      typeof file.sha256 !== 'string' ||
      !/^[a-f0-9]{64}$/.test(file.sha256) ||
      !Number.isSafeInteger(file.size) ||
      file.size < 0
    )
      throw new Error('Invalid manifest file entry');
    paths.add(file.path);
  }
  const actual = files(join(directory, 'Tau.app'), 'Tau.app');
  const ordered = [...manifest.files].sort((a, b) =>
    a.path < b.path ? -1 : a.path > b.path ? 1 : 0,
  );
  const actualOrdered = [...actual].sort((a, b) =>
    a.path < b.path ? -1 : a.path > b.path ? 1 : 0,
  );
  if (
    actualOrdered.length !== ordered.length ||
    actualOrdered.some(
      (f, i) =>
        f.path !== ordered[i]?.path ||
        f.sha256 !== ordered[i]?.sha256 ||
        f.size !== ordered[i]?.size,
    )
  )
    throw new Error('Candidate file integrity mismatch');
  const contents = join(directory, 'Tau.app/Contents');
  if (
    readFileSync(join(contents, 'Info.plist'), 'utf8') !==
      plist(expected.version, false, 'production') ||
    readFileSync(join(contents, 'Resources/tau-build.json'), 'utf8') !==
      JSON.stringify(provenance(expected), null, 2) + '\n' ||
    paths.has('Tau.app/Contents/Resources/tau-native-preview.json')
  )
    throw new Error('Plist/provenance mismatch');
  exec(['/usr/bin/plutil', '-lint', join(contents, 'Info.plist')]);
  if (verifyBinary(join(contents, 'MacOS/tau'), exec) !== 'arm64')
    throw new Error('Candidate requires arm64 only');
  return manifest;
}

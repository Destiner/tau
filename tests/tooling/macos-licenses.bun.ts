import { afterAll, expect, test } from 'bun:test';
import { createHash } from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { collectDependencyLicenses } from '../../scripts/macos/licenses';

const temp = realpathSync(mkdtempSync(join(tmpdir(), 'tau-license-tests-')));
const mit = readFileSync(join(import.meta.dir, '../../LICENSE'), 'utf8');
const digest = (data: string | Buffer): string =>
  createHash('sha256').update(data).digest('hex');
let sequence = 0;
function put(path: string, content: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
}
type FixturePackage = {
  id: string;
  name: string;
  version: string;
  manifest_path: string;
  source: string | null;
  license: string | null;
  license_file: string | null;
};
type Node = {
  id: string;
  deps: Array<{ pkg: string; dep_kinds: Array<{ kind: string | null }> }>;
};
type Index = {
  packages: Array<{
    name: string;
    version: string;
    spdx: string | null;
    sourceUrl: string;
    lockChecksum?: string;
    notices: Array<{
      name: string;
      path: string;
      sha256: string;
      origin: string;
    }>;
  }>;
};
type AddOptions = {
  kind?: string | null;
  parent?: string;
  version?: string;
  license?: string | null;
  text?: string | null;
};
type Fixture = {
  root: string;
  repo: string;
  output: string;
  metadata: {
    packages: FixturePackage[];
    resolve: { root: string; nodes: Node[] };
  };
  add: (
    name: string,
    options?: AddOptions,
  ) => { p: FixturePackage; base: string };
  run: (destination?: string) => void;
  index: (destination?: string) => Index;
};
function fixture(): Fixture {
  const root = join(temp, `fixture-${sequence++}`);
  const repo = join(root, 'repo');
  put(join(repo, 'LICENSE'), mit);
  const packages = [
    {
      id: 'root',
      name: 'tau-gpui',
      version: '0.3.0',
      manifest_path: join(repo, 'gpui/Cargo.toml'),
      source: null as string | null,
      license: 'MIT' as string | null,
      license_file: null as string | null,
    },
  ];
  const nodes: Array<{
    id: string;
    deps: Array<{ pkg: string; dep_kinds: Array<{ kind: string | null }> }>;
  }> = [{ id: 'root', deps: [] }];
  const metadata = { packages, resolve: { root: 'root', nodes } };
  const output = join(root, 'Legal');
  function add(
    name: string,
    options: AddOptions = {},
  ): { p: FixturePackage; base: string } {
    const version = options.version ?? '1.0.0';
    const id = `${name}-${version}`;
    const base = join(root, 'public-source', id);
    const p = {
      id,
      name,
      version,
      manifest_path: join(base, 'Cargo.toml'),
      source: 'registry+https://github.com/rust-lang/crates.io-index',
      license: options.license === undefined ? 'MIT' : options.license,
      license_file: null as string | null,
    };
    put(
      p.manifest_path,
      `[package]\nname = "${name}"\nversion = "${version}"\n`,
    );
    if (options.text !== null) put(join(base, 'LICENSE'), options.text ?? mit);
    packages.push(p);
    nodes.push({ id, deps: [] });
    nodes
      .find((n) => n.id === (options.parent ?? 'root'))!
      .deps.push({
        pkg: id,
        dep_kinds: [{ kind: options.kind === undefined ? null : options.kind }],
      });
    return { p, base };
  }
  function run(destination = output): void {
    put(
      join(repo, 'gpui/Cargo.lock'),
      '# fixture lock\n' +
        packages
          .filter((p) => p.source)
          .map(
            (p) =>
              `[[package]]\nname = "${p.name}"\nversion = "${p.version}"\nsource = "${p.source}"\nchecksum = "${'a'.repeat(64)}"\n`,
          )
          .join('\n'),
    );
    collectDependencyLicenses(metadata, destination, repo);
  }
  function index(destination = output): Index {
    return JSON.parse(
      readFileSync(join(destination, 'RustDependencies/index.json'), 'utf8'),
    ) as Index;
  }
  return { root, repo, metadata, output, add, run, index };
}
afterAll(() => rmSync(temp, { recursive: true, force: true }));

test('exact normal/build closure, including mixed dev edges, excludes dev-only transitive graph', () => {
  const f = fixture();
  const normal = f.add('normal');
  f.add('build-helper', { kind: 'build' });
  const dev = f.add('dev-only', {
    kind: 'dev',
    text: null,
    license: 'Unknown-9.0',
  });
  f.add('dev-transitive', { parent: dev.p.id, text: null });
  f.add('nested-build', { parent: normal.p.id, kind: 'build' });
  f.metadata.resolve.nodes[0]!.deps[0]!.dep_kinds.push({ kind: 'dev' });
  f.run();
  expect(f.index().packages.map((p) => p.name)).toEqual([
    'build-helper',
    'nested-build',
    'normal',
    'tau-gpui',
  ]);
  expect(f.index().packages.find((p) => p.name === 'normal')!.sourceUrl).toBe(
    'https://crates.io/api/v1/crates/normal/1.0.0/download',
  );
  expect(
    f.index().packages.find((p) => p.name === 'normal')!.lockChecksum,
  ).toBe('a'.repeat(64));
});

test('recursive NOTICE, copyright and all license-directory texts; no binary/test asset copying', () => {
  const f = fixture();
  const { base } = f.add('example');
  put(join(base, 'vendor/nested/NOTICE.txt'), 'Upstream attribution\n');
  put(
    join(base, 'licenses/nested/special-name.txt'),
    'Separate upstream notice\n',
  );
  put(join(base, 'src/COPYRIGHT'), 'Copyright upstream authors\n');
  put(join(base, 'tests/huge.bin'), '\0'.repeat(1024));
  put(join(base, '.cargo/config.toml'), 'PRIVATE CONFIG MUST NOT BE READ');
  f.run();
  const p = f.index().packages.find((p) => p.name === 'example')!;
  expect(p.notices.map((n) => n.name).sort()).toEqual(
    [
      'LICENSE',
      'licenses/nested/special-name.txt',
      'src/COPYRIGHT',
      'vendor/nested/NOTICE.txt',
    ].sort(),
  );
  for (const n of p.notices)
    expect(
      digest(readFileSync(join(f.output, 'RustDependencies', n.path))),
    ).toBe(n.sha256);
  expect(JSON.stringify(f.index())).not.toContain(f.root);
  expect(JSON.stringify(f.index())).not.toContain('manifest_path');
});

test('deterministic graph/notice ordering and content-addressed monorepo deduplication', () => {
  const f = fixture();
  f.add('zeta');
  f.add('alpha');
  f.run();
  const first = readFileSync(
    join(f.output, 'RustDependencies/index.json'),
    'utf8',
  );
  f.metadata.packages.reverse();
  f.metadata.resolve.nodes.reverse();
  for (const node of f.metadata.resolve.nodes) node.deps.reverse();
  const second = join(f.root, 'second');
  f.run(second);
  expect(
    readFileSync(join(second, 'RustDependencies/index.json'), 'utf8'),
  ).toBe(first);
  expect(readdirSync(join(second, 'RustDependencies/notices'))).toEqual([
    `${digest(mit)}.txt`,
  ]);
  expect(() => f.run()).toThrow('already exists');
});

test('missing files, unknown SPDX including an OR alternative, and SPDX-only attestations fail closed', () => {
  for (const options of [
    { text: null },
    { license: 'MIT OR Unknown-9.0' },
    { text: 'SPDX-License-Identifier: MIT\n' },
    { license: 'MIT! OR Apache-2.0' },
    { license: 'MIT AND Unicode-3.0' },
    { text: '' },
  ]) {
    const f = fixture();
    f.add('missing', options);
    expect(() => f.run()).toThrow();
    expect(existsSync(join(f.output, 'RustDependencies'))).toBe(false);
  }
});

test('license_file outside the package fails even with a valid LICENSE', () => {
  for (const absolute of [false, true]) {
    const f = fixture();
    const { p } = f.add('escape');
    const outside = join(f.root, 'public-source/outside');
    put(outside, mit);
    p.license_file = absolute ? outside : '../outside';
    expect(() => f.run()).toThrow('outside package');
  }
});

test('license symlinks and symlinked ancestors fail; irrelevant symlink trees are never followed', () => {
  for (const mode of ['license', 'ancestor', 'license-directory']) {
    const f = fixture();
    const { p, base } = f.add('links');
    const external = join(f.root, 'outside');
    put(join(external, 'MIT.txt'), mit);
    if (mode === 'license')
      symlinkSync(join(external, 'MIT.txt'), join(base, 'NOTICE'));
    if (mode === 'ancestor') {
      symlinkSync(external, join(base, 'subdir'));
      p.license_file = 'subdir/MIT.txt';
    }
    if (mode === 'license-directory')
      symlinkSync(external, join(base, 'licenses'));
    expect(() => f.run()).toThrow('symlink');
  }
  const f = fixture();
  const { base } = f.add('safe');
  symlinkSync(f.root, join(base, 'irrelevant-loop'));
  f.run();
  expect(f.index().packages).toHaveLength(2);
});

test('custom in-package license_file works without SPDX and absolute in-package filenames are redacted', () => {
  const f = fixture();
  const { p, base } = f.add('custom', { license: null, text: null });
  put(
    join(base, 'terms.txt'),
    'Copyright author\nCustom distribution permission.\n',
  );
  p.license_file = join(base, 'terms.txt');
  f.run();
  expect(f.index().packages[0]!.notices[0]!.name).toBe('terms.txt');
  expect(JSON.stringify(f.index())).not.toContain(f.root);
});

test('checked-in exact-version diagram notices fill an upstream omission', () => {
  const f = fixture();
  f.add('example', { text: null });
  put(
    join(
      f.repo,
      'gpui/src/components/diagram/licenses/example-1.0.0/LICENSES/MIT.txt',
    ),
    mit,
  );
  f.run();
  expect(f.index().packages[0]!.notices[0]!.origin).toBe(
    'checked-in diagram notices',
  );
});

test('immutable supplemental notices require matching crate revision and exact hashes', () => {
  for (const mode of ['valid', 'revision', 'hash', 'mutable-url']) {
    const f = fixture();
    const { base } = f.add('example', { text: null });
    const revision = 'b'.repeat(40);
    put(
      join(base, '.cargo_vcs_info.json'),
      JSON.stringify({ git: { sha1: revision } }),
    );
    const path = 'upstream-monorepo/LICENSE-MIT';
    const supplement = join(f.repo, 'gpui/assets/dependency-licenses');
    put(join(supplement, path), mit);
    put(
      join(supplement, 'provenance.json'),
      JSON.stringify({
        schemaVersion: 1,
        packages: [
          {
            name: 'example',
            version: '1.0.0',
            revision: mode === 'revision' ? 'c'.repeat(40) : revision,
            files: [
              {
                path,
                url: `https://raw.githubusercontent.com/public/upstream/${mode === 'mutable-url' ? 'main' : revision}/LICENSE-MIT`,
                sha256: mode === 'hash' ? 'd'.repeat(64) : digest(mit),
              },
            ],
          },
        ],
      }),
    );
    if (mode === 'valid') {
      f.run();
      expect(f.index().packages[0]!.notices[0]!.origin).toContain(revision);
    } else expect(() => f.run()).toThrow();
  }
});

function standardFixture(): {
  f: Fixture;
  p: FixturePackage;
  base: string;
  canonical: string;
  supplement: {
    name: string;
    version: string;
    binding: string;
    checksum: string;
    expectedRevision: string | null;
    declaredLicense: string;
    gapClassification: string;
    files: Array<{
      path: string;
      url: string;
      sha256: string;
      sourceType: string;
      spdx: string;
    }>;
  };
  save: () => void;
  supplementRoot: string;
} {
  const f = fixture();
  const { p, base } = f.add('omitted-notice', { text: null });
  const canonical = readFileSync(
    join(
      import.meta.dir,
      '../../gpui/assets/dependency-licenses/spdx--license-list-data--d46e94e2c78ceede1cfc63cfa0396472d2798d4c/MIT.txt',
    ),
    'utf8',
  );
  const supplement = {
    name: p.name,
    version: p.version,
    binding: 'registry-checksum',
    checksum: 'a'.repeat(64),
    expectedRevision: null as string | null,
    declaredLicense: 'MIT',
    gapClassification: 'archive-notice-absent-published-SPDX-standard-grant',
    files: [
      {
        path: 'canonical/MIT.txt',
        url: `https://raw.githubusercontent.com/spdx/license-list-data/${'b'.repeat(40)}/text/MIT.txt`,
        sha256: digest(canonical),
        sourceType: 'standard-grant',
        spdx: 'MIT',
      },
    ],
  };
  const supplementRoot = join(f.repo, 'gpui/assets/dependency-licenses');
  put(join(supplementRoot, 'canonical/MIT.txt'), canonical);
  const save = (): void =>
    put(
      join(supplementRoot, 'provenance.json'),
      JSON.stringify({ packages: [supplement] }),
    );
  save();
  return { f, p, base, canonical, supplement, save, supplementRoot };
}

test('checksum-bound published standard terms preserve placeholders, author metadata and every existing notice', () => {
  const { f, p, base, canonical } = standardFixture();
  Object.assign(p, {
    authors: ['Published Author'],
    repository: 'https://example.org/public-source',
  });
  const attribution = 'Copyright 1998 The Actual Published Holder\n';
  put(join(base, 'COPYRIGHT'), attribution);
  put(join(base, 'NOTICE'), 'Additional provided attribution\n');
  f.run();
  const entry = f.index().packages.find((e) => e.name === p.name)!;
  const contents = entry.notices.map((n) =>
    readFileSync(join(f.output, 'RustDependencies', n.path), 'utf8'),
  );
  expect(contents).toContain(canonical);
  expect(canonical).toContain('Copyright (c) <year> <copyright holders>');
  expect(contents).toContain(attribution);
  expect(contents).toContain('Additional provided attribution\n');
  expect(entry).toMatchObject({
    declaredAuthors: ['Published Author'],
    gapClassifications: ['archive-notice-absent-published-SPDX-standard-grant'],
  });
  expect(entry.notices).toContainEqual(
    expect.objectContaining({
      sourceType: 'standard-grant',
      provenance: expect.objectContaining({
        binding: 'registry-checksum',
        registryChecksum: 'a'.repeat(64),
        expectedRevision: null,
      }),
    }),
  );
});

test('standard fallback rejects wrong version, checksum, substituted revision, declaration and unknown SPDX', () => {
  for (const scenario of [
    'version',
    'checksum',
    'revision',
    'declaration',
    'unknown',
    'hash',
    'mutable-url',
  ]) {
    const { f, p, base, supplement, save } = standardFixture();
    if (scenario === 'version') supplement.version = '9.9.9';
    if (scenario === 'checksum') supplement.checksum = 'b'.repeat(64);
    if (scenario === 'revision')
      put(
        join(base, '.cargo_vcs_info.json'),
        JSON.stringify({ git: { sha1: 'c'.repeat(40) } }),
      );
    if (scenario === 'declaration') supplement.declaredLicense = 'Apache-2.0';
    if (scenario === 'unknown')
      p.license = supplement.declaredLicense = 'MIT OR Unknown-9.0';
    if (scenario === 'hash') supplement.files[0]!.sha256 = 'c'.repeat(64);
    if (scenario === 'mutable-url')
      supplement.files[0]!.url =
        'https://raw.githubusercontent.com/spdx/license-list-data/main/text/MIT.txt';
    save();
    expect(() => f.run()).toThrow();
    expect(existsSync(join(f.output, 'RustDependencies'))).toBe(false);
  }
});

test('registry source copyright excerpts require exact source hash and verbatim bytes', () => {
  for (const scenario of ['valid', 'source-hash', 'invented-copyright']) {
    const { f, p, base, supplement, supplementRoot } = standardFixture();
    const source =
      '// Copyright 2001 Provided Holder\n// All rights reserved.\n\npub fn example() {}\n';
    const excerpt =
      scenario === 'invented-copyright'
        ? '// Copyright 2026 Invented Holder\n'
        : source.split('\n\n')[0]! + '\n';
    put(join(base, 'src/lib.rs'), source);
    put(join(supplementRoot, 'headers/lib.txt'), excerpt);
    const sourceFile = {
      path: 'headers/lib.txt',
      url: `https://crates.io/api/v1/crates/${p.name}/${p.version}/download`,
      sha256: digest(excerpt),
      sourceType: 'crate-source-excerpt',
      sourcePath: 'src/lib.rs',
      sourceSha256:
        scenario === 'source-hash' ? 'f'.repeat(64) : digest(source),
    };
    put(
      join(supplementRoot, 'provenance.json'),
      JSON.stringify({
        packages: [{ ...supplement, files: [...supplement.files, sourceFile] }],
      }),
    );
    if (scenario === 'valid') {
      f.run();
      expect(
        f.index().packages.find((e) => e.name === p.name)!.notices,
      ).toContainEqual(
        expect.objectContaining({ sourceType: 'crate-source-excerpt' }),
      );
    } else expect(() => f.run()).toThrow();
  }
});

test('prepared upstream versions retain source checksum and patch references; unknown versions fail', () => {
  for (const version of ['0.3.7', '9.0.0']) {
    const f = fixture();
    const { p } = f.add('gpui-pre', { version });
    p.source = null;
    p.manifest_path = join(f.repo, 'gpui/.cache/crates/gpui-pre/Cargo.toml');
    put(p.manifest_path, '[package]\nname = "gpui-pre"\n');
    put(join(dirname(p.manifest_path), 'LICENSE'), mit);
    put(
      join(f.repo, 'gpui/assets/dependency-licenses/provenance.json'),
      JSON.stringify({
        packages: [],
        preparedSources: [
          { name: 'gpui-pre', version: '0.3.7', checksum: 'c'.repeat(64) },
        ],
      }),
    );
    if (version === '9.0.0')
      expect(() => f.run()).toThrow('Unmapped prepared crate version');
    else {
      f.run();
      const entry = f.index().packages.find((p) => p.name === 'gpui-pre');
      expect(entry).toMatchObject({
        modified: true,
        upstreamArchiveSha256: 'c'.repeat(64),
        checksumProvenance: 'gpui/scripts/prepare.ts',
        patches: '../gpui/patches',
        preparation: '../gpui/scripts/prepare.ts',
      });
      expect(entry).not.toHaveProperty('lockChecksum');
    }
  }
});

test('Tau-only graph uses the root MIT without recursing into repository caches', () => {
  const f = fixture();
  put(join(f.repo, 'gpui/.cache/unrelated/LICENSE'), '\0');
  f.run();
  expect(f.index().packages.map((p) => p.name)).toEqual(['tau-gpui']);
});

test('missing graph nodes and unmapped local third parties cannot silently disappear', () => {
  const f = fixture();
  const { p } = f.add('external-local');
  p.source = null;
  expect(() => f.run()).toThrow('Unmapped');
  f.metadata.resolve.nodes.pop();
  expect(() => f.run()).toThrow('Incomplete');
});

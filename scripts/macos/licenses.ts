import { createHash } from 'node:crypto';
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from 'node:fs';
import { dirname, isAbsolute, join, parse, relative, resolve } from 'node:path';

type Package = {
  id: string;
  name: string;
  version: string;
  manifest_path: string;
  source: string | null;
  license: string | null;
  license_file: string | null;
  authors?: string[];
  repository?: string | null;
};
type Metadata = {
  packages: Package[];
  resolve: {
    root: string;
    nodes: Array<{
      id: string;
      deps: Array<{ pkg: string; dep_kinds: Array<{ kind: string | null }> }>;
    }>;
  };
};
type Supplement = {
  name: string;
  version: string;
  revision?: string;
  binding?: 'registry-checksum';
  checksum?: string;
  expectedRevision?: string | null;
  declaredLicense?: string;
  gapClassification?: string;
  files: Array<{
    path: string;
    url: string;
    sha256: string;
    sourceType?: 'standard-grant' | 'crate-source-excerpt';
    spdx?: string;
    incorporatedBy?: string;
    sourcePath?: string;
    sourceSha256?: string;
  }>;
};
type Notice = {
  name: string;
  data: Buffer;
  origin: string;
  sourceType?: string;
  provenance?: object;
};
const noticeName =
  /^(\.?licen[sc]es?|copying|copyright|notices?|unlicense)([._-]|$)/i;
const prepared = new Set([
  'gpui-base',
  'gpui-pre',
  'gpui-pre-macos',
  'merman-analysis',
  'merman-core',
  'merman-layout-elk',
  'merman-render',
]);
const tauPaths: Record<string, string> = {
  'tau-gpui': 'gpui',
  'tau-core': 'crates/tau-core',
  'tau-session': 'crates/tau-session',
  'tau-observability': 'crates/tau-observability',
};
const hash = (data: Buffer): string =>
  createHash('sha256').update(data).digest('hex');
const compare = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);

// Check every component, not just the leaf: a license_file must not escape via a symlink.
function confined(base: string, path: string): string {
  const root = resolve(base);
  const target = resolve(root, path);
  const rel = relative(root, target);
  if (isAbsolute(rel) || rel === '..' || rel.startsWith('../'))
    throw new Error('License path outside package');
  let ancestor = parse(root).root;
  for (const part of relative(ancestor, root).split('/').filter(Boolean)) {
    ancestor = join(ancestor, part);
    if (lstatSync(ancestor).isSymbolicLink())
      throw new Error('License ancestor symlink forbidden');
  }
  let current = root;
  for (const part of ['', ...rel.split('/').filter(Boolean)]) {
    current = join(current, part);
    if (lstatSync(current).isSymbolicLink())
      throw new Error('License symlink forbidden');
  }
  return target;
}
function text(base: string, path: string, origin: string): Notice {
  const file = confined(base, path);
  const stat = lstatSync(file);
  if (!stat.isFile() || stat.size === 0 || stat.size > 2 * 1024 * 1024)
    throw new Error('Expected bounded nonempty license text');
  const data = readFileSync(file);
  if (data.includes(0) || !data.toString('utf8').trim())
    throw new Error('Expected license text, not binary or blank data');
  return { name: relative(resolve(base), file), data, origin };
}
function discover(
  base: string,
  origin: string,
  prefix = '',
  licenseDirectory = false,
): Notice[] {
  confined(base, prefix);
  return readdirSync(join(base, prefix))
    .sort()
    .flatMap((name): Notice[] => {
      const path = prefix ? `${prefix}/${name}` : name;
      const stat = lstatSync(join(base, path));
      if (stat.isSymbolicLink()) {
        if (licenseDirectory || noticeName.test(name))
          throw new Error('License symlink forbidden');
        return [];
      }
      if (stat.isDirectory()) {
        if (['.git', '.cargo', 'target', 'node_modules'].includes(name))
          return [];
        return discover(
          base,
          origin,
          path,
          licenseDirectory || noticeName.test(name),
        );
      }
      return licenseDirectory || noticeName.test(name)
        ? [text(base, path, origin)]
        : [];
    });
}

// This is a deliberately bounded expression/text gate, not a legal certification.
const grants: Record<string, RegExp> = {
  MIT: /permission\s+is\s+hereby\s+granted,?\s+free\s+of\s+charge/i,
  'Apache-2.0': /Apache License[\s\S]*Version 2\.0[\s\S]*TERMS AND CONDITIONS/i,
  'BSD-2-Clause':
    /redistribution and use in source and binary forms[\s\S]*THIS SOFTWARE IS PROVIDED/i,
  'BSD-3-Clause':
    /redistribution and use in source and binary forms[\s\S]*THIS SOFTWARE IS PROVIDED/i,
  '0BSD':
    /permission to use, copy, modify, and(?:\/or)?[\s\S]*for\s+any\s+purpose\s+with\s+or\s+without\s+fee/i,
  ISC: /permission to use, copy, modify, and(?:\/or)?[\s\S]*for\s+any\s+purpose\s+with\s+or\s+without\s+fee/i,
  Zlib: /origin of this software must not be misrepresented/i,
  'MPL-2.0': /Mozilla Public License[\s\S]*2\.0[\s\S]*Definitions/i,
  'EPL-2.0': /Eclipse Public License[\s\S]*2\.0[\s\S]*DEFINITIONS/i,
  'Unicode-3.0': /UNICODE[\s\S]*permission is hereby granted/i,
  Unlicense: /free and unencumbered software released into the public domain/i,
  'BSL-1.0': /Boost Software License[\s\S]*1\.0/i,
  'CC0-1.0': /CC0[\s\S]*statement of purpose/i,
  'bzip2-1.0.6':
    /bzip2[\s\S]*redistribution and use in source and binary forms/i,
};
function checkExpression(expression: string | null, notices: Notice[]): void {
  if (!expression) {
    if (!notices.length)
      throw new Error('Missing license expression and license_file');
    return;
  }
  const normalized = expression.replace(/\//g, ' OR ');
  const tokens = normalized.match(/[A-Za-z0-9.+-]+|[()]/g) ?? [];
  if (!tokens.length || tokens.join('') !== normalized.replace(/\s/g, ''))
    throw new Error('Invalid license expression');
  const body = notices.map((n) => n.data.toString('utf8')).join('\n');
  let index = 0;
  function atom(): boolean {
    const token = tokens[index++];
    if (token === '(') {
      const result = or();
      if (tokens[index++] !== ')')
        throw new Error('Invalid license expression');
      return result;
    }
    if (!token || !grants[token])
      throw new Error(`Unknown SPDX license: ${token}`);
    const result = grants[token]!.test(body);
    if (tokens[index] === 'WITH') {
      index++;
      if (tokens[index++] !== 'LLVM-exception')
        throw new Error('Unknown SPDX exception');
      return result && /LLVM Exceptions to the Apache 2.0 License/i.test(body);
    }
    return result;
  }
  function and(): boolean {
    let value = atom();
    while (tokens[index] === 'AND') {
      index++;
      const next = atom();
      value = value && next;
    }
    return value;
  }
  function or(): boolean {
    let value = and();
    while (tokens[index] === 'OR') {
      index++;
      const next = and();
      value = value || next;
    }
    return value;
  }
  if (!or() || index !== tokens.length)
    throw new Error('Missing complete license text for expression');
}

/** Caller supplies locked Cargo metadata already filtered to the distribution target.
 * Writes only RustDependencies below destination (the bundle's Legal directory).
 * No processes, network access, source-tree writes, or executable downloads.
 */
export function collectDependencyLicenses(
  metadata: unknown,
  destination: string,
  repositoryRoot: string,
): void {
  const m = metadata as Metadata;
  if (
    !Array.isArray(m?.packages) ||
    !m.resolve?.root ||
    !Array.isArray(m.resolve.nodes)
  )
    throw new Error('Expected resolved Cargo metadata with one root');
  const packages = new Map(m.packages.map((p) => [p.id, p]));
  const nodes = new Map(m.resolve.nodes.map((n) => [n.id, n]));
  const selected = new Set<string>();
  function visit(id: string): void {
    if (selected.has(id)) return;
    if (!packages.has(id) || !nodes.has(id))
      throw new Error('Incomplete Cargo dependency graph');
    selected.add(id);
    for (const dep of nodes.get(id)!.deps) {
      if (!dep.dep_kinds.length) throw new Error('Missing dependency kind');
      if (dep.dep_kinds.some((k) => k.kind === null || k.kind === 'build'))
        visit(dep.pkg);
      if (dep.dep_kinds.some((k) => ![null, 'build', 'dev'].includes(k.kind)))
        throw new Error('Unknown Cargo dependency kind');
    }
  }
  visit(m.resolve.root);
  const root = resolve(repositoryRoot);
  const supplementRoot = join(root, 'gpui/assets/dependency-licenses');
  const provenancePath = join(supplementRoot, 'provenance.json');
  const supplements = existsSync(provenancePath)
    ? (
        JSON.parse(
          text(supplementRoot, 'provenance.json', 'supplement').data.toString(),
        ) as { packages: Supplement[] }
      ).packages
    : [];
  const preparedProvenance = existsSync(provenancePath)
    ? ((
        JSON.parse(
          text(supplementRoot, 'provenance.json', 'supplement').data.toString(),
        ) as {
          preparedSources?: Array<{
            name: string;
            version: string;
            checksum: string;
          }>;
        }
      ).preparedSources ?? [])
    : [];
  const lock = Bun.TOML.parse(
    text(root, 'gpui/Cargo.lock', 'lock').data.toString(),
  ) as {
    package: Array<{
      name: string;
      version: string;
      source?: string;
      checksum?: string;
    }>;
  };
  const blobs = new Map<string, Buffer>();
  const errors: string[] = [];
  const entries: object[] = [];
  for (const id of [...selected].sort((a, b) => {
    const x = packages.get(a)!;
    const y = packages.get(b)!;
    return compare(`${x.name}@${x.version}`, `${y.name}@${y.version}`);
  })) {
    const p = packages.get(id)!;
    try {
      if (
        !/^[a-zA-Z0-9_-]+$/.test(p.name) ||
        !/^[a-zA-Z0-9.+-]+$/.test(p.version)
      )
        throw new Error('Unsafe package identity');
      const base = dirname(p.manifest_path);
      const tau =
        !p.source &&
        tauPaths[p.name] !== undefined &&
        resolve(base) === join(root, tauPaths[p.name]!);
      const patched =
        !p.source &&
        prepared.has(p.name) &&
        resolve(base) === join(root, 'gpui/.cache/crates', p.name);
      if (
        !tau &&
        !patched &&
        !p.source?.startsWith(
          'registry+https://github.com/rust-lang/crates.io-index',
        )
      )
        throw new Error('Unmapped non-crates.io dependency');
      const vcsPath = join(base, '.cargo_vcs_info.json');
      const revision = existsSync(vcsPath)
        ? (
            JSON.parse(
              text(base, '.cargo_vcs_info.json', 'crate').data.toString(),
            ) as { git?: { sha1?: string } }
          ).git?.sha1
        : undefined;
      if (revision !== undefined && !/^[0-9a-f]{40}$/.test(revision))
        throw new Error('Invalid crate VCS revision');
      const notices = tau
        ? [text(root, 'LICENSE', 'Tau root MIT')]
        : discover(base, 'crate');
      if (p.license_file) {
        // Validate even when ordinary LICENSE files already exist.
        const notice = text(base, p.license_file, 'crate license_file');
        if (!notices.some((n) => n.name === notice.name)) notices.push(notice);
      }
      const diagram = `gpui/src/components/diagram/licenses/${p.name}-${p.version}`;
      if (!tau && existsSync(join(root, diagram)))
        notices.push(
          ...discover(confined(root, diagram), 'checked-in diagram notices'),
        );
      const lockChecksum = (lock.package ?? []).find(
        (entry) =>
          entry.name === p.name &&
          entry.version === p.version &&
          entry.source === p.source,
      )?.checksum;
      const gapClassifications: string[] = [];
      for (const s of supplements.filter(
        (s) => s.name === p.name && s.version === p.version,
      )) {
        if (s.binding === 'registry-checksum') {
          if (!p.source || !s.checksum || s.checksum !== lockChecksum)
            throw new Error('Supplement registry checksum mismatch');
          if (s.expectedRevision !== (revision ?? null))
            throw new Error('Supplement crate revision mismatch');
          if (!s.declaredLicense || s.declaredLicense !== p.license)
            throw new Error('Supplement published declaration mismatch');
        } else if (!s.revision || s.revision !== revision) {
          throw new Error('Supplement crate revision mismatch');
        }
        if (s.gapClassification) gapClassifications.push(s.gapClassification);
        for (const file of s.files) {
          const immutable =
            /^https:\/\/raw\.githubusercontent\.com\/[^/]+\/[^/]+\/[0-9a-f]{40}\//.test(
              file.url,
            );
          if (file.sourceType === 'standard-grant') {
            if (!immutable || !file.spdx || !grants[file.spdx])
              throw new Error('Invalid pinned standard grant');
            if (s.binding !== 'registry-checksum')
              throw new Error(
                'Standard grant requires registry checksum binding',
              );
          } else if (file.sourceType === 'crate-source-excerpt') {
            if (
              s.binding !== 'registry-checksum' ||
              !file.sourcePath ||
              !file.sourceSha256 ||
              file.url !==
                `https://crates.io/api/v1/crates/${p.name}/${p.version}/download`
            )
              throw new Error('Invalid crate source excerpt provenance');
            const source = text(base, file.sourcePath, 'crate source');
            if (hash(source.data) !== file.sourceSha256)
              throw new Error('Crate source excerpt hash mismatch');
          } else if (
            !immutable ||
            !s.revision ||
            !file.url.includes(`/${s.revision}/`)
          ) {
            throw new Error(
              'Supplement must reference immutable public revision',
            );
          }
          const notice = text(supplementRoot, file.path, file.url);
          if (hash(notice.data) !== file.sha256)
            throw new Error('Supplement notice hash mismatch');
          if (
            file.sourceType === 'standard-grant' &&
            !grants[file.spdx!]!.test(notice.data.toString('utf8'))
          )
            throw new Error('Standard grant text mismatch');
          if (
            file.sourceType === 'crate-source-excerpt' &&
            !text(base, file.sourcePath!, 'crate source').data.includes(
              notice.data,
            )
          )
            throw new Error('Source excerpt is not verbatim');
          notice.sourceType = file.sourceType ?? 'upstream-revision-notice';
          notice.provenance = {
            binding: s.binding ?? 'crate-vcs-revision',
            ...(s.binding
              ? {
                  registryChecksum: s.checksum,
                  expectedRevision: s.expectedRevision,
                  declaredLicense: s.declaredLicense,
                }
              : { revision: s.revision }),
            ...(file.spdx ? { standardLicense: file.spdx } : {}),
            ...(file.incorporatedBy
              ? { incorporatedBy: file.incorporatedBy }
              : {}),
            ...(file.sourcePath
              ? { sourcePath: file.sourcePath, sourceSha256: file.sourceSha256 }
              : {}),
          };
          notices.push(notice);
        }
      }
      if (!notices.length)
        throw new Error('Missing distribution license/copyright notices');
      if (!p.license && !p.license_file && !tau)
        throw new Error('Missing declared license');
      checkExpression(tau ? 'MIT' : p.license, notices);
      const upstreamArchiveSha256 = patched
        ? preparedProvenance.find(
            (entry) => entry.name === p.name && entry.version === p.version,
          )?.checksum
        : undefined;
      if (patched && !upstreamArchiveSha256)
        throw new Error('Unmapped prepared crate version');
      if (!tau && !lockChecksum && !upstreamArchiveSha256 && !revision)
        throw new Error('Missing lock checksum or crate VCS revision');
      for (const checksum of [lockChecksum, upstreamArchiveSha256])
        if (checksum && !/^[0-9a-f]{64}$/.test(checksum))
          throw new Error('Invalid source archive checksum');
      const indexed = notices
        .map((notice) => {
          const sha256 = hash(notice.data);
          blobs.set(sha256, notice.data);
          return {
            name: notice.name,
            origin: notice.origin,
            sourceType: notice.sourceType ?? 'provided-notice',
            ...(notice.provenance ? { provenance: notice.provenance } : {}),
            path: `notices/${sha256}.txt`,
            sha256,
          };
        })
        .sort((a, b) => compare(JSON.stringify(a), JSON.stringify(b)));
      entries.push({
        name: p.name,
        version: p.version,
        spdx: tau ? 'MIT' : p.license,
        declaredAuthors: p.authors ?? [],
        declaredRepository: p.repository ?? null,
        gapClassifications: [...new Set(gapClassifications)].sort(compare),
        sourceUrl: tau
          ? 'https://github.com/destiner/tau'
          : `https://crates.io/api/v1/crates/${p.name}/${p.version}/download`,
        ...(lockChecksum ? { lockChecksum } : {}),
        ...(upstreamArchiveSha256
          ? {
              upstreamArchiveSha256,
              checksumProvenance: 'gpui/scripts/prepare.ts',
            }
          : {}),
        ...(revision ? { crateVcsRevision: revision } : {}),
        ...(patched
          ? {
              modified: true,
              patches: '../gpui/patches',
              preparation: '../gpui/scripts/prepare.ts',
            }
          : {}),
        notices: indexed,
      });
    } catch (error) {
      errors.push(
        `${p.name}@${p.version}: ${error instanceof Error ? error.message : 'Notice collection failed'}`,
      );
    }
  }
  if (errors.length)
    throw new Error(
      `Rust dependency notice blockers (${errors.length}):\n${errors.join('\n')}`,
    );
  const output = join(destination, 'RustDependencies');
  if (existsSync(output))
    throw new Error('RustDependencies destination already exists');
  mkdirSync(join(output, 'notices'), { recursive: true });
  for (const [sha256, data] of [...blobs].sort(([a], [b]) => compare(a, b)))
    writeFileSync(join(output, 'notices', `${sha256}.txt`), data);
  writeFileSync(
    join(output, 'index.json'),
    `${JSON.stringify({ schemaVersion: 2, packages: entries }, null, 2)}\n`,
  );
  writeFileSync(
    join(output, 'README.txt'),
    'Distribution notices and immutable upstream source links for the selected normal/build Cargo dependency graph. Dev-only edges are excluded. SPDX expressions are published declarations, not legal certification. declaredAuthors is factual Cargo author metadata, not an assertion of copyright ownership. sourceType distinguishes provided notices, exact-revision upstream notices, canonical standard grants and verbatim source excerpts; a standard grant is not a recovered crate-specific copyright notice. Canonical placeholders remain unchanged. gapClassifications records unavailable crate-specific notices, not missing known terms or a guarantee that no other notices exist. All discovered license, copyright and NOTICE files are preserved; alternative license texts may also be included. Versioned crates.io downloads identify upstream sources, including unmodified MPL/EPL crates. Modified prepared crates also require the patches and corresponding source already bundled elsewhere in Legal; this collector does not replace them.\n',
  );
}

import { createHash } from 'node:crypto';
import {
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
  writeFile,
} from 'node:fs/promises';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const root = fileURLToPath(new URL('../', import.meta.url));
const crates = [
  {
    name: 'gpui-base',
    version: '0.7.0',
    checksum:
      '64bb52b29a8dcdc8d3f595b8b0839c076f237572ab601ba4577d3da8ae085728',
    patches: [
      'grapheme-navigation.patch',
      'textarea-rows.patch',
      'disabled-focus.patch',
      'input-presentation.patch',
      'single-line-navigation.patch',
      'textarea-selection.patch',
      'input-scroll-sensitivity.patch',
      'composer-caret-reveal.patch',
      'textarea-vertical-edges.patch',
      'paragraph-navigation.patch',
      'code-block-scroll.patch',
      'product-markdown-style.patch',
      'markdown-strong-style.patch',
      'markdown-list-style.patch',
      'markdown-inline-spacing.patch',
      'markdown-prose-measure.patch',
      'fractional-leading-selection-test.patch',
      'virtualized-text-selection.patch',
      'markdown-parity.patch',
      'native-menu-base.patch',
      'ime-composition.patch',
      'focus-trap-accessibility.patch',
      'disabled-control-accessibility.patch',
      'input-editor-accessibility.patch',
      'bounded-modal-focus.patch',
      'dialog-top-layer-dismissal.patch',
      'editor-values.patch',
    ],
  },
  {
    name: 'gpui-pre',
    version: '0.3.7',
    checksum:
      '0e87a42bb37c7cb4e76dd1ac0ce88851e46e976e0373a47ab3e0757abffee54d',
    patches: [
      'fractional-text-leading.patch',
      'estimated-list-height.patch',
      'native-menu-core.patch',
      'window-close-request.patch',
      'accessibility-test-platform.patch',
      'disabled-control-accessibility-core.patch',
      'input-editor-accessibility-core.patch',
    ],
  },
  {
    name: 'gpui-pre-macos',
    version: '0.3.7',
    checksum:
      '5a43af845b260b09393e923c4f1e1a67a10fcfc847b4815192bbfd02ed9fe725',
    patches: ['antialiased-text.patch', 'grayscale-text.patch'],
  },
  {
    name: 'merman-core',
    version: '0.8.0',
    checksum:
      'c8375d78a1c09f5ad1c847d8d18658f5dbe4221c31b9eb56d4a3dc121184f9d4',
    patches: [
      'merman-core-parser-version.patch',
      'merman-core-er-direction.patch',
    ],
  },
  {
    name: 'merman-analysis',
    version: '0.8.0',
    checksum:
      '9359567480a01600cfe8f17811ffcf7d51df17697e95f6e37bd1ba7d5aefbdde',
    patches: ['merman-analysis-parser-version.patch'],
  },
  {
    name: 'merman-render',
    version: '0.8.0',
    checksum:
      'bbb640e1bb9e614384724f9f9efae431a6faf464c34b95e74d56200fcbfd6c7b',
    patches: [
      'merman-render-class-er.patch',
      'merman-render-state-sequence.patch',
      'merman-render-flowchart.patch',
      'merman-render-polish.patch',
    ],
  },
  {
    name: 'merman-layout-elk',
    version: '0.8.0',
    checksum:
      '4ce75a18461574fd6a20c13dafdc0f865250ad4423e13d1274eff0f41295d8b0',
    patches: ['merman-layout-elk-options.patch'],
  },
] as const;
type Crate = (typeof crates)[number];
const sha = (value: Uint8Array | string): string =>
  createHash('sha256').update(value).digest('hex');

export function verifyArchive(
  bytes: Uint8Array,
  config: Crate = crates[0],
): void {
  if (sha(bytes) !== config.checksum)
    throw new Error(`${config.name} archive checksum mismatch`);
}

async function sourceHash(path: string): Promise<string> {
  const hash = createHash('sha256');
  async function visit(directory: string): Promise<void> {
    for (const entry of (
      await readdir(directory, { withFileTypes: true })
    ).sort((a, b) => a.name.localeCompare(b.name))) {
      if (['target', 'Cargo.lock', '.tau-patch.json'].includes(entry.name))
        continue;
      const child = join(directory, entry.name);
      if (entry.isDirectory()) await visit(child);
      else if (entry.isFile())
        hash
          .update(child.slice(path.length))
          .update('\0')
          .update(await readFile(child));
      else throw new Error(`unexpected dependency source entry: ${entry.name}`);
    }
  }
  await visit(path);
  return hash.digest('hex');
}

async function archive(config: Crate): Promise<Uint8Array> {
  const crate = `${config.name}-${config.version}`;
  const registry = join(
    process.env.CARGO_HOME || join(homedir(), '.cargo'),
    'registry/cache',
  );
  for (const directory of await readdir(registry).catch(() => [])) {
    const bytes = await readFile(
      join(registry, directory, `${crate}.crate`),
    ).catch(() => undefined);
    if (bytes && sha(bytes) === config.checksum) return bytes;
  }
  const response = await fetch(
    `https://static.crates.io/crates/${config.name}/${crate}.crate`,
  );
  if (!response.ok)
    throw new Error(`${config.name} download failed: HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  verifyArchive(bytes, config);
  return bytes;
}

async function prepareCrate(config: Crate): Promise<void> {
  const { patches, checksum } = config;
  const crate = `${config.name}-${config.version}`;
  const patchBytes = await Promise.all(
    patches.map((p) => readFile(join(root, 'patches', p))),
  );
  const fingerprint = sha(
    Buffer.concat([Buffer.from(`v2:${checksum}`), ...patchBytes]),
  );
  const target = resolve(root, `.cache/crates/${config.name}`);
  const stamp = join(target, '.tau-patch.json');
  const previous = await readFile(stamp, 'utf8')
    .then((s) => JSON.parse(s) as { fingerprint: string; source: string })
    .catch(() => undefined);
  if (
    previous?.fingerprint === fingerprint &&
    previous.source === (await sourceHash(target))
  )
    return;

  // git apply silently skips unprefixed paths when run inside this repository's subdirectories.
  const temporary = await mkdtemp(join(tmpdir(), 'tau-gpui-prepare-'));
  try {
    const bytes = await archive(config);
    verifyArchive(bytes, config);
    const tarball = join(temporary, `${crate}.crate`);
    await writeFile(tarball, bytes);
    execFileSync('tar', ['-xzf', tarball, '-C', temporary]);
    const source = join(temporary, crate);
    for (const patch of patches) {
      execFileSync('git', ['apply', '--check', join(root, 'patches', patch)], {
        cwd: source,
      });
      execFileSync('git', ['apply', join(root, 'patches', patch)], {
        cwd: source,
      });
    }
    await writeFile(
      join(source, '.tau-patch.json'),
      JSON.stringify({ fingerprint, source: await sourceHash(source) }),
    );
    await mkdir(dirname(target), { recursive: true });
    const staged = `${target}-${process.pid}`;
    await cp(source, staged, { recursive: true });
    await rm(target, { recursive: true, force: true });
    await rename(staged, target);
    if (config.name === 'gpui-base') {
      // Published Base tests still include the workspace README from outside their crate.
      await writeFile(
        join(root, '.cache/README.md'),
        await readFile(join(target, 'README.md')),
      );
    }
    console.log(
      `Prepared checksum-verified ${crate} with ${patches.length} focused patches`,
    );
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

export async function prepare(): Promise<void> {
  for (const config of crates) await prepareCrate(config);
}

if (import.meta.main) await prepare();

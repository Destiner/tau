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
const checksum =
  '64bb52b29a8dcdc8d3f595b8b0839c076f237572ab601ba4577d3da8ae085728';
const crate = 'gpui-base-0.7.0';
const patches = [
  'grapheme-navigation.patch',
  'textarea-rows.patch',
  'disabled-focus.patch',
];
const sha = (value: Uint8Array | string): string =>
  createHash('sha256').update(value).digest('hex');

export function verifyArchive(bytes: Uint8Array): void {
  if (sha(bytes) !== checksum)
    throw new Error('gpui-base archive checksum mismatch');
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

async function archive(): Promise<Uint8Array> {
  const registry = join(
    process.env.CARGO_HOME || join(homedir(), '.cargo'),
    'registry/cache',
  );
  for (const directory of await readdir(registry).catch(() => [])) {
    const bytes = await readFile(
      join(registry, directory, `${crate}.crate`),
    ).catch(() => undefined);
    if (bytes && sha(bytes) === checksum) return bytes;
  }
  const response = await fetch(
    `https://static.crates.io/crates/gpui-base/${crate}.crate`,
  );
  if (!response.ok)
    throw new Error(`gpui-base download failed: HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  verifyArchive(bytes);
  return bytes;
}

export async function prepare(): Promise<void> {
  const patchBytes = await Promise.all(
    patches.map((p) => readFile(join(root, 'patches', p))),
  );
  const fingerprint = sha(
    Buffer.concat([Buffer.from(`v2:${checksum}`), ...patchBytes]),
  );
  const target = resolve(root, '.cache/crates/gpui-base');
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
    const bytes = await archive();
    verifyArchive(bytes);
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
    // Published Base tests still include the workspace README from outside their crate.
    await writeFile(
      join(root, '.cache/README.md'),
      await readFile(join(target, 'README.md')),
    );
    console.log(
      `Prepared checksum-verified ${crate} with ${patches.length} focused patches`,
    );
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

if (import.meta.main) await prepare();

import { createHash, randomUUID } from 'node:crypto';
import { lstat, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { PNG } from 'pngjs';
import { exec } from './native-test-helper';
import { prepare } from './prepare';
import { hashPaths } from './provenance';

const root = resolve(import.meta.dir, '../..');
export const probeIds = [
  'markdown',
  'menus',
  'dialogs',
  'composer',
  'models',
  'palette',
  'queue',
  'activity',
  'files',
] as const;
export type ProbeId = (typeof probeIds)[number];
type Metadata = {
  id: string;
  title: string;
  description: string;
  limitations: string;
  width: number;
  height: number;
  checks: string[];
};
type Check = {
  name: string;
  passed: boolean;
  observations: unknown;
  screenshot: string;
  ink_pixels: number;
};
type Proof = {
  runId: string;
  id: string;
  theme: string;
  sourceHash: string;
  binaryHash: string;
  complete: boolean;
  artifacts: Record<string, string>;
};
const sha = (bytes: Uint8Array): string =>
  createHash('sha256').update(bytes).digest('hex');
const safeName = (name: unknown): name is string =>
  typeof name === 'string' && /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(name);
const record = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === 'object' && !Array.isArray(v);
export const escapeHtml = (text: string): string =>
  text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');

export function validateMetadata(value: unknown, id: ProbeId): Metadata {
  if (
    !record(value) ||
    value.id !== id ||
    !['title', 'description', 'limitations'].every(
      (key) => typeof value[key] === 'string',
    ) ||
    !Number.isInteger(value.width) ||
    !Number.isInteger(value.height) ||
    (value.width as number) < 1 ||
    (value.height as number) < 1 ||
    (value.width as number) > 2000 ||
    (value.height as number) > 2000 ||
    !Array.isArray(value.checks) ||
    value.checks.length < 1 ||
    value.checks.length > 100 ||
    !value.checks.every(safeName) ||
    new Set(value.checks).size !== value.checks.length
  )
    throw new Error(`Invalid ${id} binary metadata`);
  return value as Metadata;
}
export function validateChecks(value: unknown, metadata: Metadata): Check[] {
  if (!Array.isArray(value) || value.length !== metadata.checks.length)
    throw new Error('Incomplete probe checks');
  for (let i = 0; i < value.length; i++) {
    const c: unknown = value[i];
    const name = metadata.checks[i];
    if (
      !record(c) ||
      c.name !== name ||
      c.passed !== true ||
      c.screenshot !== `${name}.png` ||
      !Number.isSafeInteger(c.ink_pixels) ||
      (c.ink_pixels as number) <= 400 ||
      (c.ink_pixels as number) > metadata.width * metadata.height * 4 ||
      !record(c.observations)
    )
      throw new Error(`Failed or malformed probe check: ${name}`);
  }
  return value as Check[];
}
async function regularBytes(path: string, limit: number): Promise<Buffer> {
  const stat = await lstat(path);
  if (!stat.isFile() || stat.size > limit)
    throw new Error(`Unsafe or oversized artifact: ${path}`);
  return readFile(path);
}
async function binaryHash(id: ProbeId): Promise<string> {
  return sha(await regularBytes(binaryPath(id), 150_000_000));
}
function binaryPath(id: ProbeId): string {
  return join(root, 'gpui/target/debug', `${id}-probe`);
}
export async function sourceHash(): Promise<string> {
  return hashPaths(root, [
    'gpui/src/',
    'gpui/assets/',
    'gpui/patches/',
    'gpui/Cargo.toml',
    'gpui/Cargo.lock',
  ]);
}
export async function loadMetadata(id: ProbeId): Promise<Metadata> {
  const run = await exec(binaryPath(id), ['--list-checks']);
  if (run.code) throw new Error(`${id} --list-checks failed: ${run.output}`);
  return validateMetadata(JSON.parse(run.output), id);
}
export async function artifactHashes(
  dir: string,
  metadata: Metadata,
): Promise<Record<string, string>> {
  const manifest = await regularBytes(join(dir, 'checks.json'), 1_000_000);
  const checks = validateChecks(JSON.parse(manifest.toString()), metadata);
  const hashes: Record<string, string> = { 'checks.json': sha(manifest) };
  for (const check of checks) {
    const bytes = await regularBytes(join(dir, check.screenshot), 25_000_000);
    // Bound allocation before PNG.sync.read inflates attacker-controlled dimensions.
    if (
      bytes.length < 24 ||
      bytes.subarray(0, 8).toString('hex') !== '89504e470d0a1a0a' ||
      bytes.readUInt32BE(16) !== metadata.width * 2 ||
      bytes.readUInt32BE(20) !== metadata.height * 2
    )
      throw new Error(`Invalid image dimensions: ${check.screenshot}`);
    const image = PNG.sync.read(bytes);
    if (
      image.width !== metadata.width * 2 ||
      image.height !== metadata.height * 2
    )
      throw new Error(`Invalid decoded image dimensions: ${check.screenshot}`);
    const background = image.data.subarray(0, 3);
    let ink = 0;
    for (let pixel = 0; pixel < image.width * image.height; pixel++) {
      const offset = pixel * 4;
      if (
        [0, 1, 2].some(
          (channel) =>
            Math.abs(image.data[offset + channel]! - background[channel]!) > 8,
        )
      )
        ink++;
    }
    if (ink !== check.ink_pixels)
      throw new Error(`Ink pixel count mismatch: ${check.screenshot}`);
    hashes[check.screenshot] = sha(bytes);
  }
  return hashes;
}
async function proofAt(dir: string): Promise<Proof> {
  return JSON.parse(
    (await regularBytes(join(dir, 'capture.json'), 100_000)).toString(),
  ) as Proof;
}
export function validateProof(
  proof: Proof,
  expected: {
    id: ProbeId;
    theme: string;
    sourceHash: string;
    binaryHash: string;
  },
  artifacts: Record<string, string>,
): void {
  if (
    !record(proof) ||
    proof.complete !== true ||
    proof.id !== expected.id ||
    proof.theme !== expected.theme ||
    typeof proof.runId !== 'string' ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      proof.runId,
    ) ||
    proof.sourceHash !== expected.sourceHash ||
    proof.binaryHash !== expected.binaryHash ||
    !record(proof.artifacts) ||
    JSON.stringify(proof.artifacts) !== JSON.stringify(artifacts)
  )
    throw new Error(
      `Incomplete, stale or tampered ${expected.id} ${expected.theme} capture; recapture`,
    );
}
async function verified(
  base: string,
  id: ProbeId,
  metadata: Metadata,
  inputs: string,
  binary: string,
): Promise<Record<string, Check[]>> {
  const themes: Record<string, Check[]> = {};
  const runIds = new Set<string>();
  for (const theme of ['light', 'dark']) {
    const dir = join(base, id, theme);
    const proof = await proofAt(dir);
    const artifacts = await artifactHashes(dir, metadata);
    validateProof(
      proof,
      { id, theme, sourceHash: inputs, binaryHash: binary },
      artifacts,
    );
    if (runIds.has(proof.runId)) throw new Error('Reused capture run ID');
    runIds.add(proof.runId);
    themes[theme] = validateChecks(
      JSON.parse(
        (await regularBytes(join(dir, 'checks.json'), 1_000_000)).toString(),
      ),
      metadata,
    );
  }
  return themes;
}
export async function captureProbe(
  base: string,
  id: ProbeId,
  metadata: Metadata,
  inputs: string,
): Promise<void> {
  const binary = await binaryHash(id);
  await rm(join(base, 'index.html'), { force: true });
  await rm(join(base, id, 'report.html'), { force: true });
  for (const theme of ['light', 'dark']) {
    const dir = join(base, id, theme);
    await rm(dir, { recursive: true, force: true });
    await mkdir(dir, { recursive: true });
    const proof: Proof = {
      runId: randomUUID(),
      id,
      theme,
      sourceHash: inputs,
      binaryHash: binary,
      complete: false,
      artifacts: {},
    };
    await writeFile(join(dir, 'capture.json'), JSON.stringify(proof));
    const result = await exec(binaryPath(id), [
      '--capture',
      dir,
      ...(theme === 'dark' ? ['--dark'] : []),
    ]);
    if (result.code)
      throw new Error(`${id} ${theme} capture failed: ${result.output}`);
    proof.artifacts = await artifactHashes(dir, metadata);
    if (inputs !== (await sourceHash()) || binary !== (await binaryHash(id)))
      throw new Error(`${id} changed during capture`);
    proof.complete = true;
    await writeFile(join(dir, 'capture.json'), JSON.stringify(proof, null, 2));
    console.log(result.output.trim());
  }
}
export const featuredChecks: Record<ProbeId, readonly string[]> = {
  markdown: ['mixed-code', 'long-scrolled', 'tall-scrolled'],
  menus: ['dropdown', 'stacked', 'edge'],
  dialogs: ['nested-open', 'edit-busy', 'quit-busy'],
  composer: ['draft', 'busy-guard', 'long-wrap'],
  models: ['selected-visible', 'filtered', 'failure'],
  palette: ['search-insertion', 'filtered', 'nested'],
  queue: ['hover-open', 'preview-retained', 'reordered'],
  activity: ['detail-scrolled', 'failed-local', 'many-tail'],
  files: ['code-ready', 'long-scrolled', 'image-ready'],
};

export function embeddedChecks(metadata: Metadata): readonly string[] {
  const featured = featuredChecks[metadata.id as ProbeId];
  return featured?.length === 3 &&
    featured.every((name) => metadata.checks.includes(name))
    ? featured
    : metadata.checks.slice(0, 3);
}

const html = (title: string, body: string): string =>
  `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${escapeHtml(title)}</title><style>body{font:15px/1.5 system-ui;max-width:960px;margin:32px auto;padding:0 20px;background:#f4f5f7;color:#252a32}article{background:white;padding:18px;margin:20px 0;border-radius:8px}img{max-width:100%;height:auto}pre{white-space:pre-wrap;overflow-wrap:anywhere}a{color:#245d98}h2{font-size:19px}</style>${body}</html>`;
export function renderProbe(
  metadata: Metadata,
  themes: Record<string, Check[]>,
): string {
  const embedded = new Set(embeddedChecks(metadata));
  const sections = ['light', 'dark']
    .map((theme) => {
      const checks = themes[theme];
      if (!checks) throw new Error(`Missing ${theme} evidence`);
      return `<section id="${theme}"><h2>${theme === 'light' ? 'Light' : 'Dark'} · ${checks.length} checks passed</h2>${checks.map((c) => `<article><h3>${escapeHtml(c.name)} · PASS</h3><p>${c.ink_pixels} non-background pixels</p><pre>${escapeHtml(JSON.stringify(c.observations, null, 2))}</pre><a href="${theme}/${c.screenshot}">${embedded.has(c.name) ? `<img loading="lazy" width="${metadata.width}" height="${metadata.height}" src="${theme}/${c.screenshot}" alt="Native ${theme} ${escapeHtml(c.name)} frame">` : 'View full frame'}</a></article>`).join('')}</section>`;
    })
    .join('');
  return html(
    `${metadata.title} · native probe`,
    `<nav><a href="../index.html">All probes</a> · <a href="#light">Light</a> · <a href="#dark">Dark</a></nav><h1>${escapeHtml(metadata.title)}</h1><p><strong>Native-only focused probe; not Vue parity.</strong> Isolated GPUI Metal capture with synthetic content.</p><p>${escapeHtml(metadata.description)}</p><p><strong>Limitations:</strong> ${escapeHtml(metadata.limitations)} Whole-frame ink coverage only detects near-blank frames; it does not establish semantic visual equivalence. Hashes and labels are verified at report generation, not continuously while viewing.</p><p>Launch interactive window: <code>cd gpui &amp;&amp; cargo run --bin ${metadata.id}-probe</code>. No user session is involved; Markdown checks exercise the headless test clipboard.</p>${sections}`,
  );
}
export async function reportProbes(
  base: string,
  ids: readonly ProbeId[] = probeIds,
): Promise<void> {
  await rm(join(base, 'index.html'), { force: true });
  const inputs = await sourceHash();
  const entries: string[] = [];
  const requested = new Set(ids);
  const failures: string[] = [];
  for (const id of probeIds) {
    const report = join(base, id, 'report.html');
    await rm(report, { force: true });
    let current = false;
    try {
      const metadata = await loadMetadata(id);
      const checks = await verified(
        base,
        id,
        metadata,
        inputs,
        await binaryHash(id),
      );
      await writeFile(report, renderProbe(metadata, checks));
      current = true;
    } catch (error) {
      await rm(report, { force: true });
      if (requested.has(id))
        failures.push(
          `${id}: ${error instanceof Error ? error.message : String(error)}`,
        );
    }
    entries.push(
      `<li>${current ? `<a href="${id}/report.html">${id}</a> · current verified light/dark capture` : `${id} · no current verified report`}</li>`,
    );
  }
  await mkdir(base, { recursive: true });
  await writeFile(
    join(base, 'index.html'),
    html(
      'Native UI probes',
      `<h1>Native UI probes</h1><p>Focused native-only captures, not Vue parity. Only current complete evidence is linked.</p><ul>${entries.join('')}</ul><p>Run <code>bun gpui/scripts/probe-report.ts --capture</code> to refresh captures.</p>`,
    ),
  );
  console.log(join(base, 'index.html'));
  if (failures.length)
    throw new Error(
      `Unable to report requested probe(s): ${failures.join('; ')}`,
    );
}
export async function main(args: string[]): Promise<void> {
  let capture = false;
  let id: ProbeId | undefined;
  let out = resolve(root, 'gpui/results/ui-probes');
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--capture' && !capture) capture = true;
    else if (
      arg === '--probe' &&
      !id &&
      probeIds.includes(args[i + 1] as ProbeId)
    )
      id = args[++i] as ProbeId;
    else if (arg === '--out' && args[i + 1] && !args[i + 1]!.startsWith('--'))
      out = resolve(args[++i]!);
    else
      throw new Error(
        'Usage: probe-report.ts [--capture] [--probe ID] [--out directory]',
      );
  }
  const ids = id ? [id] : probeIds;
  if (capture) {
    await prepare();
    const build = await exec('cargo', ['build', '--locked', '--bins']);
    if (build.code) throw new Error(build.output);
    const inputs = await sourceHash();
    for (const target of ids)
      await captureProbe(out, target, await loadMetadata(target), inputs);
  }
  await reportProbes(out, ids);
}
if (import.meta.main) await main(process.argv.slice(2));

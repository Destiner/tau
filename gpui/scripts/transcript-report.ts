import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { PNG } from 'pngjs';
import { hashPaths, nativeBinaryHash } from './provenance';
import { prepare } from './prepare';
import { exec } from './native-test-helper';

const root = resolve(import.meta.dir, '../..');
export const transcriptChecks = [
  'initial',
  'follow',
  'growth',
  'shrink',
  'stream',
  'pause',
  'wheel',
  'collapse',
  'reader',
  'above',
  'latest',
  'large',
  'large-growth',
  'reset',
  'interrupted',
  'bounded',
];
const pinChecks = new Set([
  'follow',
  'growth',
  'shrink',
  'stream',
  'pause',
  'latest',
  'large',
  'large-growth',
  'bounded',
]);
type Check = {
  name: string;
  passed: boolean;
  detail: string;
  screenshot: string;
  geometry?: {
    viewport_bottom: number;
    last_row_bottom: number;
    gap: number;
  } | null;
};
type Proof = {
  runId: string;
  theme: string;
  inputs: string;
  binary: string;
  complete: boolean;
  files: Record<string, string>;
};
const digest = (data: Uint8Array): string =>
  createHash('sha256').update(data).digest('hex');
const escape = (s: string): string =>
  s
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');

export function validateTranscriptChecks(value: unknown): Check[] {
  if (!Array.isArray(value) || value.length !== transcriptChecks.length)
    throw new Error('Incomplete transcript checks');
  const checks = value as Check[];
  for (const name of transcriptChecks) {
    const matches = checks.filter((c) => c?.name === name);
    if (matches.length !== 1)
      throw new Error(`Transcript check must occur exactly once: ${name}`);
    const c = matches[0]!;
    if (
      c.passed !== true ||
      typeof c.detail !== 'string' ||
      c.screenshot !== `${name}.png`
    )
      throw new Error(`Failed or invalid transcript check: ${name}`);
    if (pinChecks.has(name)) {
      const g = c.geometry;
      if (
        !g ||
        ![g.viewport_bottom, g.last_row_bottom, g.gap].every(Number.isFinite) ||
        g.viewport_bottom <= 0 ||
        g.gap < 0 ||
        g.gap > 1 ||
        Math.abs(Math.abs(g.viewport_bottom - g.last_row_bottom) - g.gap) >
          0.001
      )
        throw new Error(
          `Missing or inconsistent measured end-pin geometry: ${name}`,
        );
    }
  }
  return checks;
}
async function inputsHash(): Promise<string> {
  return hashPaths(root, [
    'gpui/src/transcript.rs',
    'gpui/src/bin/transcript-probe.rs',
    'gpui/src/text.rs',
    'gpui/Cargo.toml',
    'gpui/Cargo.lock',
    'gpui/assets/',
    'gpui/patches/',
  ]);
}
async function filesHash(dir: string): Promise<Record<string, string>> {
  const files: Record<string, string> = {};
  const manifest = await readFile(join(dir, 'checks.json'));
  files['checks.json'] = digest(manifest);
  const checks = validateTranscriptChecks(JSON.parse(manifest.toString()));
  for (const check of checks) {
    const bytes = await readFile(join(dir, check.screenshot));
    const image = PNG.sync.read(bytes);
    if (image.width !== 1520 || image.height !== 1240)
      throw new Error(`Invalid Metal image dimensions: ${check.screenshot}`);
    files[check.screenshot] = digest(bytes);
  }
  return files;
}
export async function captureTranscript(base: string): Promise<void> {
  await prepare();
  const build = await exec('cargo', [
    'build',
    '--locked',
    '--bin',
    'transcript-probe',
  ]);
  if (build.code !== 0) throw new Error(build.output);
  for (const theme of ['light', 'dark']) {
    const dir = join(base, theme);
    await mkdir(dir, { recursive: true });
    const proof: Proof = {
      runId: randomUUID(),
      theme,
      inputs: await inputsHash(),
      binary: await nativeBinaryHash('transcript-probe'),
      complete: false,
      files: {},
    };
    await writeFile(join(dir, 'capture.json'), JSON.stringify(proof));
    const capture = await exec(
      join(root, 'gpui/target/debug/transcript-probe'),
      ['--capture', dir, ...(theme === 'dark' ? ['--dark'] : [])],
    );
    if (capture.code !== 0) throw new Error(capture.output);
    proof.files = await filesHash(dir);
    if (
      proof.inputs !== (await inputsHash()) ||
      proof.binary !== (await nativeBinaryHash('transcript-probe'))
    )
      throw new Error('Transcript changed during capture');
    proof.complete = true;
    await writeFile(join(dir, 'capture.json'), JSON.stringify(proof, null, 2));
    console.log(capture.output.trim());
  }
}
const reviewNotes: Record<string, string> = {
  follow:
    'Append and grow in the same frame. The bottom of the final measured row stays within one logical pixel of the viewport bottom.',
  reader:
    'Reading an older message while new content arrives. Its stable identity and physical row bounds are preserved. A programmatically established within-message selection retains its selected text; this is not yet a live pointer-selection/paint regression.',
  above:
    'A row above the reader grows from its measured original height; the visible reader anchor and selection do not move. Returning to the row preserves its expansion state.',
  shrink:
    'A synthetic 110px footer shrinks the viewport, representing a composer growing. End-follow remains pinned using measured geometry.',
  large:
    'A 1,000-message variable-height fixture uses the GPUI list. This demonstrates rendering and navigation, not a sustained-performance benchmark.',
  reset:
    'Reset restores the small fixture, stops streaming and discards only this explicitly synthetic demo state.',
};
export async function transcriptReport(
  base = resolve(root, 'gpui/results/transcript'),
): Promise<void> {
  const inputs = await inputsHash(),
    binary = await nativeBinaryHash('transcript-probe');
  let sections = '';
  for (const theme of ['light', 'dark']) {
    const dir = join(base, theme);
    const proof: Proof = JSON.parse(
      await readFile(join(dir, 'capture.json'), 'utf8'),
    );
    if (
      !proof.complete ||
      !proof.runId ||
      proof.theme !== theme ||
      proof.inputs !== inputs ||
      proof.binary !== binary
    )
      throw new Error(
        `Incomplete or stale ${theme} transcript evidence; run --capture`,
      );
    const files = await filesHash(dir);
    if (JSON.stringify(files) !== JSON.stringify(proof.files))
      throw new Error(`Changed ${theme} transcript evidence; recapture`);
    const checks = validateTranscriptChecks(
      JSON.parse(await readFile(join(dir, 'checks.json'), 'utf8')),
    );
    const cards = Object.entries(reviewNotes).map(([name, note]) => {
      const c = checks.find((c) => c.name === name)!;
      return `<article><h2>${escape(name)} · PASS</h2><p>${escape(note)}</p><details><summary>Observed evidence</summary><pre>${escape(c.detail)}</pre></details><a href="${theme}/${c.screenshot}"><img width="760" height="620" src="${theme}/${c.screenshot}" alt="Native ${theme} ${escape(name)}"></a></article>`;
    });
    sections += `<section data-theme="${theme}" ${theme === 'dark' ? 'hidden' : ''}><p>16 behavioral checks passed. Six representative frames below; <a href="${theme}/checks.json">all observed checks</a>.</p>${cards.join('')}</section>`;
  }
  await writeFile(
    join(base, 'report.html'),
    `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Streaming transcript probe</title><style>body{font:15px/1.5 system-ui;max-width:900px;margin:28px auto;padding:0 20px;background:#f4f5f7;color:#272e38}article{background:white;padding:16px;margin:20px 0;border-radius:8px}img{max-width:100%;height:auto}pre{white-space:pre-wrap;overflow-wrap:anywhere}h2{font-size:18px}[hidden]{display:none!important}select{font:inherit;padding:5px}</style><h1>Streaming transcript · native feasibility</h1><p><strong>Native-only, not Vue parity.</strong> Real measured GPUI list geometry and Metal screenshots. Selection-state retention, variable-height rows, stable IDs, streaming, expansion and end-follow are exercised with synthetic content.</p><p>Live: Start/Pause runs a bounded 200ms stream; Step works while paused. Scroll up while streaming, try Grow Above Reader, Collapse/Expand, Shrink Viewport, Jump to Latest and Large (1,000). Reset stops the stream. Links are deliberately inert in this isolated probe.</p><p>Not yet established: pointer-selection paint retention, cross-message selection, full Markdown/media compatibility, real-window performance or accessibility. The resize test changes a synthetic footer, not the actual application's composer. No Pi or user data is involved.</p><label>Theme <select id="theme"><option value="light">Light</option><option value="dark">Dark</option></select></label>${sections}<script>document.querySelector('#theme').addEventListener('change',event=>{for(const section of document.querySelectorAll('[data-theme]')) section.hidden=section.dataset.theme!==event.target.value;});</script></html>`,
  );
  console.log(join(base, 'report.html'));
}
if (import.meta.main) {
  const args = process.argv.slice(2);
  const capture = args[0] === '--capture';
  if (capture) args.shift();
  if (args.length > 1 || args[0]?.startsWith('--'))
    throw new Error(
      'Usage: transcript-report.ts [--capture] [output-directory]',
    );
  const base = resolve(args[0] ?? join(root, 'gpui/results/transcript'));
  if (capture) await captureTranscript(base);
  await transcriptReport(base);
}

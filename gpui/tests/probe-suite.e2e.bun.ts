import { beforeAll, expect, test } from 'bun:test';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { exec } from '../scripts/native-test-helper';
import { prepare } from '../scripts/prepare';
import { assertProbePaint } from './probe-paint';
import {
  captureProbe,
  featuredChecks,
  loadMetadata,
  probeIds,
  reportProbes,
  sourceHash,
} from '../scripts/probe-report';

const binaries = resolve(import.meta.dir, '../target/debug');
beforeAll(async () => {
  await prepare();
  const build = await exec('cargo', ['build', '--locked', '--bins']);
  if (build.code) throw new Error(build.output);
}, 600000);

test('all nine probes capture both themes and reject hide-content and skip-input without reusing proof', async () => {
  const base = await mkdtemp(join(tmpdir(), 'tau-probe-suite-'));
  try {
    const inputs = await sourceHash();
    for (const id of probeIds) {
      const metadata = await loadMetadata(id);
      for (const name of featuredChecks[id])
        expect(metadata.checks).toContain(name);
      await captureProbe(base, id, metadata, inputs);
      await assertProbePaint(base, id);
      await reportProbes(base, [id]);
      const html = await readFile(join(base, id, 'report.html'), 'utf8');
      expect(html).toContain('Native-only focused probe');
      expect(html).toContain('Light');
      expect(html).toContain('Dark');
      for (const fault of ['hide-content', 'skip-input']) {
        const dir = join(base, id, 'light');
        // Native capture clears checks.json itself, while the old complete proof remains:
        // validation must refuse to reuse it regardless of fault diagnostics.
        const run = await exec(join(binaries, `${id}-probe`), [
          '--capture',
          dir,
          '--fault',
          fault,
        ]);
        expect(run.code, `${id} ${fault}: ${run.output}`).not.toBe(0);
        await expect(reportProbes(base, [id])).rejects.toThrow(
          `Unable to report requested probe(s): ${id}:`,
        );
        expect(await readFile(join(base, 'index.html'), 'utf8')).not.toContain(
          `${id}/report.html`,
        );
        await captureProbe(base, id, metadata, inputs);
      }
    }
    await reportProbes(base);
    const index = await readFile(join(base, 'index.html'), 'utf8');
    for (const id of probeIds) expect(index).toContain(`${id}/report.html`);

    const unselected = join(base, 'menus', 'report.html');
    const original = await readFile(unselected, 'utf8');
    await writeFile(unselected, 'tampered HTML');
    await reportProbes(base, ['markdown']);
    expect(await readFile(unselected, 'utf8')).toBe(original);
    expect(await readFile(join(base, 'index.html'), 'utf8')).toContain(
      'menus/report.html',
    );

    const checksPath = join(base, 'menus', 'light', 'checks.json');
    const manifest = await readFile(checksPath, 'utf8');
    await writeFile(checksPath, `${manifest} `);
    await reportProbes(base, ['markdown']);
    await expect(readFile(unselected, 'utf8')).rejects.toThrow();
    const staleIndex = await readFile(join(base, 'index.html'), 'utf8');
    expect(staleIndex).not.toContain('menus/report.html');
    expect(staleIndex).toContain('menus · no current verified report');
    await expect(reportProbes(base, ['menus'])).rejects.toThrow(
      'menus: Incomplete, stale or tampered',
    );
    expect(await readFile(join(base, 'index.html'), 'utf8')).not.toContain(
      'menus/report.html',
    );
  } finally {
    await rm(base, { recursive: true, force: true });
  }
}, 1_800_000);

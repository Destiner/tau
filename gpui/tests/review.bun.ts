import { expect, test } from 'bun:test';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  hashPaths,
  assertCurrentCapture,
  captureArtifactsHash,
} from '../scripts/provenance';
import {
  textareaNotes,
  textareaFeedbackNotes,
  textareaRows,
} from '../scripts/component-review';
import {
  transcriptChecks,
  validateTranscriptChecks,
} from '../scripts/transcript-report';
import type { Row } from '../scripts/compare';

test('render fingerprints include nested binary sources, not directory reads or docs', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'tau-fingerprint-'));
  try {
    await mkdir(join(dir, 'src/bin'), { recursive: true });
    await writeFile(join(dir, 'src/main.rs'), 'main');
    await writeFile(join(dir, 'src/bin/probe.rs'), 'probe');
    const first = await hashPaths(dir, ['src/']);
    await writeFile(join(dir, 'src/README.md'), 'notes');
    expect(await hashPaths(dir, ['src/'])).toBe(first);
    await writeFile(join(dir, 'src/bin/probe.rs'), 'changed');
    expect(await hashPaths(dir, ['src/'])).not.toBe(first);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('component evidence binds request, observations, report rows and images but allows new HTML views', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'tau-artifacts-'));
  try {
    for (const folder of ['vue', 'native', 'report'])
      await mkdir(join(dir, folder));
    await writeFile(join(dir, 'request.json'), '{}');
    await writeFile(join(dir, 'vue/observations.json'), '[]');
    await writeFile(join(dir, 'native/observations.json'), '[]');
    await writeFile(join(dir, 'report/report.json'), '{}');
    await writeFile(join(dir, 'report/frame.png'), 'image');
    const before = await captureArtifactsHash(dir);
    await writeFile(join(dir, 'report/textarea.html'), 'new presentation');
    expect(await captureArtifactsHash(dir)).toBe(before);
    for (const path of [
      'request.json',
      'vue/observations.json',
      'native/observations.json',
      'report/report.json',
      'report/frame.png',
    ]) {
      const original = await readFile(join(dir, path));
      await writeFile(join(dir, path), 'changed');
      expect(await captureArtifactsHash(dir)).not.toBe(before);
      await writeFile(join(dir, path), original);
    }
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('focused reports require complete current source and binary evidence', () => {
  const capture = {
    runId: 'run',
    browser: 'chromium' as const,
    renderInputs: 'inputs',
    nativeBinary: 'binary',
    complete: true,
  };
  expect(() => assertCurrentCapture(capture, 'inputs', 'binary')).not.toThrow();
  for (const patch of [
    { runId: '' },
    { complete: false },
    { renderInputs: 'stale' },
    { nativeBinary: 'stale' },
  ])
    expect(() =>
      assertCurrentCapture({ ...capture, ...patch }, 'inputs', 'binary'),
    ).toThrow('completed capture');
});

test('multiline review selects only its curated cases and rejects missing/duplicate cases', () => {
  const rows = Object.keys(textareaNotes).map((id) => ({ id }) as Row);
  expect(textareaRows([...rows, { id: 'input-empty/idle' } as Row])).toEqual(
    rows,
  );
  expect(rows).toHaveLength(14);
  const followup = Object.keys(textareaFeedbackNotes).map(
    (id) => ({ id }) as Row,
  );
  expect(
    textareaRows(
      [...rows.filter((row) => !textareaFeedbackNotes[row.id]), ...followup],
      textareaFeedbackNotes,
    ),
  ).toEqual(followup);
  expect(followup).toHaveLength(5);
  expect(() => textareaRows(followup.slice(1), textareaFeedbackNotes)).toThrow(
    'exactly one',
  );
  expect(() => textareaRows(rows.slice(1))).toThrow('exactly one');
  expect(() => textareaRows([...rows, rows[0]!])).toThrow('exactly one');
});

test('native transcript report rejects false, missing, duplicate and inconsistent geometric evidence', () => {
  const checks = transcriptChecks.map((name) => ({
    name,
    passed: true,
    detail: 'observed',
    screenshot: `${name}.png`,
    geometry: { viewport_bottom: 600, last_row_bottom: 600, gap: 0 },
  }));
  expect(validateTranscriptChecks(checks)).toHaveLength(16);
  expect(() => validateTranscriptChecks(checks.slice(1))).toThrow('Incomplete');
  expect(() =>
    validateTranscriptChecks([checks[1], ...checks.slice(1)]),
  ).toThrow('exactly once');
  for (const patch of [
    { passed: false },
    { screenshot: '../escape.png' },
    { geometry: null },
    { geometry: { viewport_bottom: 600, last_row_bottom: 500, gap: 0 } },
    { geometry: { viewport_bottom: 600, last_row_bottom: 598, gap: 2 } },
  ]) {
    const changed = checks.map((c) =>
      c.name === 'follow' ? { ...c, ...patch } : c,
    );
    expect(() => validateTranscriptChecks(changed)).toThrow();
  }
});

import { expect, test } from 'bun:test';
import { createHash, randomUUID } from 'node:crypto';
import {
  mkdtemp,
  mkdir,
  readFile,
  rm,
  symlink,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PNG } from 'pngjs';
import {
  artifactHashes,
  escapeHtml,
  embeddedChecks,
  featuredChecks,
  probeIds,
  renderProbe,
  validateChecks,
  validateMetadata,
  validateProof,
} from '../scripts/probe-report';

const metadata = validateMetadata(
  {
    id: 'markdown',
    title: '<Markdown>',
    description: 'A & B',
    limitations: 'No "parity"',
    width: 16,
    height: 16,
    checks: ['first', 'next-step'],
  },
  'markdown',
);
const checks = (): {
  name: string;
  passed: boolean;
  observations: { text: string };
  screenshot: string;
  ink_pixels: number;
}[] =>
  metadata.checks.map((name) => ({
    name,
    passed: true,
    observations: { text: '<script>alert("x")</script>' },
    screenshot: `${name}.png`,
    ink_pixels: 401,
  }));
const digest = (bytes: Uint8Array): string =>
  createHash('sha256').update(bytes).digest('hex');

test('binary metadata requires bounded dimensions, ordered unique safe names and correct ID', () => {
  expect(validateMetadata(metadata, 'markdown').checks).toEqual([
    'first',
    'next-step',
  ]);
  for (const patch of [
    { checks: ['first', 'first'] },
    { checks: ['../file'] },
    { checks: [] },
    { width: 100000 },
    { id: 'files' },
  ])
    expect(() =>
      validateMetadata({ ...metadata, ...patch }, 'markdown'),
    ).toThrow();
});

test('checks reject missing, duplicate, reordered, failed, malformed and unsafe screenshot paths', () => {
  expect(validateChecks(checks(), metadata)).toHaveLength(2);
  const cases = [
    checks().slice(0, 1),
    [checks()[0], checks()[0]],
    checks().reverse(),
    checks().map((c, i) => (i ? { ...c, passed: false } : c)),
    checks().map((c, i) => (i ? { ...c, screenshot: '../evil.png' } : c)),
    checks().map((c, i) => (i ? { ...c, ink_pixels: NaN } : c)),
    checks().map((c, i) => (i ? { ...c, ink_pixels: 1025 } : c)),
    checks().map((c, i) => (i ? { ...c, observations: null } : c)),
  ];
  for (const bad of cases)
    expect(() => validateChecks(bad, metadata)).toThrow();
});

test('curation selects distinct representative frames and falls back without native artifacts', () => {
  for (const id of probeIds) {
    const featured = featuredChecks[id];
    expect(featured).toHaveLength(3);
    expect(new Set(featured).size).toBe(3);
    for (const name of featured) expect(name).not.toBe('initial');
    expect(
      embeddedChecks({
        ...metadata,
        id,
        checks: ['initial', ...featured, 'last'],
      }),
    ).toEqual(featured);
    expect(
      embeddedChecks({ ...metadata, id, checks: ['first', 'second'] }),
    ).toEqual(['first', 'second']);
  }
});

test('HTML escapes metadata and observations, embeds fallback frames and links all', () => {
  expect(escapeHtml(`<&>"'`)).toBe('&lt;&amp;&gt;&quot;&#39;');
  const rendered = renderProbe(metadata, { light: checks(), dark: checks() });
  expect(rendered).not.toContain('<script>');
  expect(rendered).not.toContain('<Markdown>');
  expect(rendered).toContain('&lt;script&gt;');
  expect(rendered).toContain('light/next-step.png');
  expect(rendered).toContain('dark/first.png');
  expect(rendered.match(/<img /g)).toHaveLength(4);
  expect(rendered).toContain('href="#light"');
  expect(rendered).toContain('href="#dark"');
  expect(rendered).toContain('<section id="light">');
  expect(rendered).toContain('<section id="dark">');
  expect(rendered).toContain('not Vue parity');
  expect(rendered).toContain('does not establish semantic visual equivalence');
  expect(rendered).toContain('not continuously while viewing');
});

test('artifacts require physical dimensions, valid PNG and exact proof hashes', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'tau-probe-report-'));
  try {
    await mkdir(dir, { recursive: true });
    const png = new PNG({ width: 32, height: 32 });
    png.data.fill(255);
    for (let pixel = 1; pixel <= 401; pixel++) png.data[pixel * 4] = 0;
    const bytes = PNG.sync.write(png);
    await writeFile(join(dir, 'checks.json'), JSON.stringify(checks()));
    for (const name of metadata.checks)
      await writeFile(join(dir, `${name}.png`), bytes);
    const artifacts = await artifactHashes(dir, metadata);
    expect(artifacts['first.png']).toBe(digest(bytes));
    const expected = {
      id: 'markdown' as const,
      theme: 'light',
      sourceHash: 'source',
      binaryHash: 'binary',
    };
    const proof = {
      ...expected,
      runId: randomUUID(),
      complete: true,
      artifacts,
    };
    expect(() => validateProof(proof, expected, artifacts)).not.toThrow();
    for (const change of [
      { complete: false },
      { sourceHash: 'stale' },
      { binaryHash: 'stale' },
      { artifacts: { ...artifacts, 'first.png': 'tampered' } },
      { runId: 'no' },
    ])
      expect(() =>
        validateProof({ ...proof, ...change }, expected, artifacts),
      ).toThrow();
    await writeFile(
      join(dir, 'first.png'),
      PNG.sync.write(new PNG({ width: 31, height: 32 })),
    );
    await expect(artifactHashes(dir, metadata)).rejects.toThrow('dimensions');
    await writeFile(join(dir, 'first.png'), bytes);
    const bogus = checks();
    bogus[0]!.ink_pixels = 402;
    await writeFile(join(dir, 'checks.json'), JSON.stringify(bogus));
    await expect(artifactHashes(dir, metadata)).rejects.toThrow(
      'Ink pixel count mismatch',
    );
    await writeFile(join(dir, 'checks.json'), JSON.stringify(checks()));
    await rm(join(dir, 'first.png'));
    await symlink(join(dir, 'next-step.png'), join(dir, 'first.png'));
    await expect(artifactHashes(dir, metadata)).rejects.toThrow('Unsafe');
    await rm(join(dir, 'first.png'));
    await writeFile(join(dir, 'first.png'), bytes);
    await writeFile(
      join(dir, 'checks.json'),
      (await readFile(join(dir, 'checks.json'), 'utf8')).replace('401', '400'),
    );
    await expect(artifactHashes(dir, metadata)).rejects.toThrow('malformed');
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

import { spawnSync, type SpawnSyncReturns } from 'node:child_process';
import {
  chmodSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, describe, expect, it } from 'vitest';

const preflight = fileURLToPath(
  new URL('./release-preflight.sh', import.meta.url),
);
const fixtures: string[] = [];

type FixtureOptions = {
  packageVersion?: string;
  tauriVersion?: string;
  cargoVersion?: string;
  releases?: string;
  refs?: string;
  failReleases?: boolean;
  failRefs?: boolean;
};

function createFixture({
  packageVersion = '1.2.3',
  tauriVersion = packageVersion,
  cargoVersion = packageVersion,
  releases = '[[]]',
  refs = '[]',
  failReleases = false,
  failRefs = false,
}: FixtureOptions = {}): {
  cwd: string;
  output: string;
  run: () => SpawnSyncReturns<string>;
} {
  const cwd = mkdtempSync(join(tmpdir(), 'tau-release-preflight-'));
  fixtures.push(cwd);
  const bin = join(cwd, 'bin');
  const output = join(cwd, 'github-output');

  mkdirSync(join(cwd, 'src-tauri'), { recursive: true });
  mkdirSync(bin);
  writeFileSync(output, '');
  writeFileSync(
    join(cwd, 'package.json'),
    JSON.stringify({ version: packageVersion }),
  );
  writeFileSync(
    join(cwd, 'src-tauri', 'tauri.conf.json'),
    JSON.stringify({ version: tauriVersion }),
  );
  writeFileSync(
    join(cwd, 'src-tauri', 'Cargo.toml'),
    `version = "${cargoVersion}"\n`,
  );
  writeFileSync(
    join(bin, 'gh'),
    `#!/usr/bin/env bash
set -eu
case "$*" in
  *'/releases?per_page=100'*)
    [[ "\${GH_FAIL_RELEASES:-}" != 1 ]] || exit 42
    printf '%s' "$GH_RELEASES"
    ;;
  *'/git/matching-refs/tags/'*)
    [[ "\${GH_FAIL_REFS:-}" != 1 ]] || exit 43
    printf '%s' "$GH_REFS"
    ;;
esac
`,
  );
  chmodSync(join(bin, 'gh'), 0o755);

  return {
    cwd,
    output,
    run: (): SpawnSyncReturns<string> =>
      spawnSync('bash', [preflight], {
        cwd,
        encoding: 'utf8',
        env: {
          ...process.env,
          GITHUB_REF: 'refs/heads/main',
          GITHUB_REPOSITORY: 'owner/tau',
          GITHUB_OUTPUT: output,
          GH_RELEASES: releases,
          GH_REFS: refs,
          GH_FAIL_RELEASES: failReleases ? '1' : '',
          GH_FAIL_REFS: failRefs ? '1' : '',
          PATH: `${bin}:${process.env.PATH}`,
        },
      }),
  };
}

afterEach(() => {
  for (const fixture of fixtures.splice(0))
    rmSync(fixture, { force: true, recursive: true });
});

describe('release preflight', () => {
  it('only permits releases dispatched from main', () => {
    const fixture = createFixture();
    const result = spawnSync('bash', [preflight], {
      cwd: fixture.cwd,
      encoding: 'utf8',
      env: { ...process.env, GITHUB_REF: 'refs/heads/feature' },
    });

    expect(result.status).toBe(1);
    expect(result.stderr).toContain('Releases must be dispatched from main.');
  });

  it('rejects prerelease versions', () => {
    const result = createFixture({ packageVersion: '1.2.3-rc.1' }).run();

    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('stable major.minor.patch');
  });

  it('rejects disagreeing package, Tauri, and Cargo versions', () => {
    const result = createFixture({ tauriVersion: '1.2.4' }).run();

    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('Release versions disagree.');
  });

  it.each([
    ['published', '[[],[{"tag_name":"v1.2.3","draft":false}]]'],
    ['draft', '[[],[{"tag_name":"v1.2.3","draft":true}]]'],
  ])('rejects a %s release found on any API page', (_kind, releases) => {
    const result = createFixture({ releases }).run();

    expect(result.status).toBe(1);
    expect(result.stderr).toContain('Release v1.2.3 already exists');
  });

  it('rejects an exact existing tag but ignores a matching prefix', () => {
    const existing = createFixture({
      refs: '[{"ref":"refs/tags/v1.2.3"}]',
    }).run();
    const prefix = createFixture({
      refs: '[{"ref":"refs/tags/v1.2.30"}]',
    }).run();

    expect(existing.status).toBe(1);
    expect(existing.stderr).toContain('Tag v1.2.3 already exists');
    expect(prefix.status).toBe(0);
  });

  it.each([
    ['release listing', { failReleases: true }],
    ['tag lookup', { failRefs: true }],
  ])('fails closed when the %s API call fails', (_operation, options) => {
    const fixture = createFixture(options);
    const result = fixture.run();

    expect(result.status).not.toBe(0);
    expect(readFileSync(fixture.output, 'utf8')).toBe('');
  });

  it('writes the validated version and tag to the GitHub output', () => {
    const fixture = createFixture();
    const result = fixture.run();

    expect(result.status).toBe(0);
    expect(result.stdout).toBe('');
    expect(readFileSync(fixture.output, 'utf8')).toBe(
      'version=1.2.3\ntag=v1.2.3\n',
    );
  });
});

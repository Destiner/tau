import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  createUpdateManifest,
  releaseArtifactNames,
  updaterPublicKeyFingerprint,
  validateReleaseAssets,
  validateUpdaterArchiveListing,
  verifyUpdaterSignature,
  writeUpdateManifest,
} from './updater-release';

const fixturePublicKey =
  'dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEFBNEIwMjMwRkEzQzc2MDQKUldRRWRqejZNQUpMcXVvRkZHSmNNTWNoUXA4OHMvWDFPMmpraXJDYnJta09zYnYxZFJsZEQxTHQK';
const fixtureSignature =
  'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRRWRqejZNQUpMcWwyYy9xQWY5S0ZRUEorMnpSS0ZmTGFzWlEvRlRsb1NIU3k5T2krY2Y5b0tMWHpJTXdMZkdFMkRwVUpqUVNmV2szUHhXVjdSekh0eFdMUURlUTdXZncwPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkwMDI3MjI1CWZpbGU6YXJjaGl2ZQpUOWNobHBYSDdpeFZQMXVLcnpXNG5OSk8vbTVMbEQ1dmFCNGNFSHU3QWFpODFpZ1A1TjNMalIyenlkQmZzMEh4MXUzNzRBMWJBMHZ3d0J1L3FNQ0tCUT09Cg==';
const temporaryDirectories: string[] = [];

function temporaryDirectory(): string {
  const directory = mkdtempSync(join(tmpdir(), 'tau-updater-test-'));
  temporaryDirectories.push(directory);
  return directory;
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe('updater release artifacts', () => {
  it('uses deterministic names and an immutable archive URL', () => {
    expect(releaseArtifactNames('1.2.3')).toEqual({
      dmg: 'tau-1.2.3-apple-silicon.dmg',
      archive: 'tau-1.2.3-darwin-aarch64.tar.gz',
      signature: 'tau-1.2.3-darwin-aarch64.tar.gz.sig',
      manifest: 'latest.json',
    });
    expect(createUpdateManifest('1.2.3', fixtureSignature)).toEqual({
      version: '1.2.3',
      platforms: {
        'darwin-aarch64': {
          url: 'https://github.com/Destiner/tau/releases/download/v1.2.3/tau-1.2.3-darwin-aarch64.tar.gz',
          signature: fixtureSignature,
        },
      },
    });
  });

  it('writes and validates exactly four corresponding assets', () => {
    const directory = temporaryDirectory();
    const names = releaseArtifactNames('1.2.3');
    writeFileSync(join(directory, names.dmg), 'dmg');
    writeFileSync(join(directory, names.archive), 'archive');
    writeFileSync(join(directory, names.signature), fixtureSignature);
    writeUpdateManifest(directory, '1.2.3');

    expect(validateReleaseAssets(directory, '1.2.3').version).toBe('1.2.3');
    const manifestPath = join(directory, names.manifest);
    const manifest = readFileSync(manifestPath, 'utf8');
    writeFileSync(manifestPath, manifest.replace('/v1.2.3/', '/latest/'));
    expect(() => validateReleaseAssets(directory, '1.2.3')).toThrow(
      'latest.json does not exactly match',
    );
    writeFileSync(manifestPath, manifest);
    writeFileSync(join(directory, 'unexpected'), '');
    expect(() => validateReleaseAssets(directory, '1.2.3')).toThrow(
      'release assets are',
    );
  });

  it('fingerprints the canonical decoded public key', () => {
    expect(updaterPublicKeyFingerprint(fixturePublicKey)).toBe(
      '71787717524340b7fb401032843ea678cdd9991747043fee5c20fc873c1788f1',
    );
  });

  it('rejects links and special archive entries before extraction', () => {
    expect(
      validateUpdaterArchiveListing(
        'Tau.app/\nTau.app/Contents/MacOS/tau\n',
        'drwxr-xr-x  0 user group 0 Jan 1 00:00 Tau.app/\n-rwxr-xr-x  0 user group 1 Jan 1 00:00 Tau.app/Contents/MacOS/tau\n',
      ),
    ).toEqual(['Tau.app/', 'Tau.app/Contents/MacOS/tau']);

    for (const type of ['l', 'h', 'p', 'c', 'b']) {
      expect(() =>
        validateUpdaterArchiveListing(
          'Tau.app/entry\n',
          `${type}rw-r--r--  0 user group 0 Jan 1 00:00 Tau.app/entry\n`,
        ),
      ).toThrow('links or special entries');
    }
  });

  it('cryptographically verifies Tauri signer output', () => {
    const directory = temporaryDirectory();
    const archive = join(directory, 'archive');
    writeFileSync(archive, 'tau updater fixture\n');
    expect(() =>
      verifyUpdaterSignature(archive, fixtureSignature, fixturePublicKey),
    ).not.toThrow();

    writeFileSync(archive, 'tampered\n');
    expect(() =>
      verifyUpdaterSignature(archive, fixtureSignature, fixturePublicKey),
    ).toThrow('invalid updater signature');
  });
});

describe('updater release configuration', () => {
  const root = resolve(import.meta.dirname, '..');

  it('keeps updater signing release-only and configures the public endpoint', () => {
    const base = JSON.parse(
      readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'),
    ) as {
      bundle: { createUpdaterArtifacts?: boolean };
      plugins: { updater: { endpoints: string[]; pubkey?: string } };
    };
    const release = JSON.parse(
      readFileSync(join(root, 'src-tauri/tauri.release.conf.json'), 'utf8'),
    ) as {
      build: { beforeBuildCommand: string };
      bundle: { createUpdaterArtifacts: boolean };
    };

    expect(base.bundle.createUpdaterArtifacts).not.toBe(true);
    expect(base.plugins.updater.pubkey).toBeFalsy();
    expect(base.plugins.updater.endpoints).toEqual([
      'https://github.com/Destiner/tau/releases/latest/download/latest.json',
    ]);
    expect(release).toEqual({
      build: { beforeBuildCommand: '' },
      bundle: { createUpdaterArtifacts: true },
    });

    const releaseScript = readFileSync(
      join(root, 'scripts/release-macos.ts'),
      'utf8',
    );
    expect(releaseScript).toContain("'app,dmg'");
    expect(releaseScript).toContain('TAU_UPDATER_PUBLIC_KEY');
  });

  it('requires updater secrets and uploads all assets without clobbering', () => {
    const workflow = readFileSync(
      join(root, '.github/workflows/release.yml'),
      'utf8',
    );
    expect(workflow).toContain('TAURI_SIGNING_PRIVATE_KEY');
    expect(workflow).toContain('TAU_UPDATER_PUBLIC_KEY');
    expect(workflow).toContain('TAURI_SIGNING_PRIVATE_KEY_PASSWORD');
    expect(workflow).toContain('Build frontend without release credentials');
    expect(
      workflow.indexOf('Build frontend without release credentials'),
    ).toBeLessThan(workflow.indexOf('Import release credentials'));
    expect(workflow).toContain('release-assets/$VERSION');
    expect(workflow).toContain('"$assets"/*');
    expect(workflow).not.toContain('--clobber');

    const releaseScript = readFileSync(
      join(root, 'scripts/release-macos.ts'),
      'utf8',
    );
    expect(releaseScript).toContain('release destination already exists');
    expect(releaseScript).toContain("['-tvzf', archive]");
    expect(releaseScript).toContain('requirePinnedUpdaterPublicKey');
    expect(releaseScript).toContain("command('bun', ['run', 'build']");
    expect(releaseScript).toContain('env: frontendEnvironment');
    expect(releaseScript).toContain("!key.startsWith('APPLE_')");
    expect(releaseScript).toContain("key !== 'TAURI_SIGNING_PRIVATE_KEY'");
    expect(releaseScript).toContain(
      "key !== 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD'",
    );
    expect(releaseScript).toContain("key !== 'TAU_UPDATER_PUBLIC_KEY'");
  });

  it('builds the frontend before the credentialed Tauri build', () => {
    const releaseScript = readFileSync(
      join(root, 'scripts/release-macos.ts'),
      'utf8',
    );
    expect(
      releaseScript.indexOf("command('bun', ['run', 'build']"),
    ).toBeLessThan(
      releaseScript.indexOf("command('bun', [\n      'tauri',\n      'build'"),
    );
  });
});

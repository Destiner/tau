import { createHash, createPublicKey, verify } from 'node:crypto';
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';

const releaseRepository = 'Destiner/tau';
const updaterPlatform = 'darwin-aarch64';

interface ReleaseArtifactNames {
  dmg: string;
  archive: string;
  signature: string;
  manifest: 'latest.json';
}

interface UpdateManifest {
  version: string;
  platforms: Record<typeof updaterPlatform, { url: string; signature: string }>;
}

function releaseArtifactNames(version: string): ReleaseArtifactNames {
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`invalid release version: ${version}`);
  }
  const stem = `tau-${version}`;
  const archive = `${stem}-darwin-aarch64.tar.gz`;
  return {
    dmg: `${stem}-apple-silicon.dmg`,
    archive,
    signature: `${archive}.sig`,
    manifest: 'latest.json',
  };
}

function immutableUpdaterUrl(version: string, archive: string): string {
  return `https://github.com/${releaseRepository}/releases/download/v${version}/${archive}`;
}

function createUpdateManifest(
  version: string,
  signature: string,
): UpdateManifest {
  const names = releaseArtifactNames(version);
  requireCanonicalEncodedFile(signature, 'updater signature');
  return {
    version,
    platforms: {
      [updaterPlatform]: {
        url: immutableUpdaterUrl(version, names.archive),
        signature,
      },
    },
  };
}

function writeUpdateManifest(directory: string, version: string): string {
  const names = releaseArtifactNames(version);
  const signature = readFileSync(join(directory, names.signature), 'utf8');
  const path = join(directory, names.manifest);
  writeFileSync(
    path,
    `${JSON.stringify(createUpdateManifest(version, signature), null, 2)}\n`,
  );
  return path;
}

function validateReleaseAssets(
  directory: string,
  version: string,
): UpdateManifest {
  const names = releaseArtifactNames(version);
  const expected = Object.values(names).sort();
  const actual = readdirSync(directory).sort();
  if (actual.join('\n') !== expected.join('\n')) {
    throw new Error(
      `release assets are ${actual.join(', ')}, expected ${expected.join(', ')}`,
    );
  }

  const signature = readFileSync(join(directory, names.signature), 'utf8');
  const manifest = JSON.parse(
    readFileSync(join(directory, names.manifest), 'utf8'),
  ) as unknown;
  const expectedManifest = createUpdateManifest(version, signature);
  if (JSON.stringify(manifest) !== JSON.stringify(expectedManifest)) {
    throw new Error('latest.json does not exactly match the release artifacts');
  }
  return expectedManifest;
}

function decodeWrappedMinisign(value: string, subject: string): string[] {
  requireCanonicalEncodedFile(value, subject);
  const decoded = Buffer.from(value, 'base64').toString('utf8');
  const lines = decoded.trimEnd().split('\n');
  if (`${decoded.trimEnd()}\n` !== decoded) {
    throw new Error(`${subject} has invalid encoded text`);
  }
  return lines;
}

function requireCanonicalEncodedFile(value: string, subject: string): void {
  if (
    !value ||
    value !== value.trim() ||
    !/^[A-Za-z0-9+/]+={0,2}$/.test(value)
  ) {
    throw new Error(`${subject} is not canonical base64 content`);
  }
  const decoded = Buffer.from(value, 'base64');
  if (decoded.toString('base64') !== value) {
    throw new Error(`${subject} is not canonical base64 content`);
  }
}

function updaterPublicKeyFingerprint(publicKeyContent: string): string {
  requireCanonicalEncodedFile(publicKeyContent, 'updater public key');
  return createHash('sha256')
    .update(Buffer.from(publicKeyContent, 'base64'))
    .digest('hex');
}

function validateUpdaterArchiveListing(
  pathListing: string,
  verboseListing: string,
): string[] {
  const entries = pathListing.split('\n').filter(Boolean);
  const typedEntries = verboseListing.split('\n').filter(Boolean);
  if (entries.length === 0 || typedEntries.length !== entries.length) {
    throw new Error('updater archive listing is empty or inconsistent');
  }

  const rejected = typedEntries.flatMap((entry, index) => {
    const type = entry[0];
    return type === '-' || type === 'd'
      ? []
      : [`${entries[index] ?? '<unknown>'} (${type ?? 'unknown'})`];
  });
  if (rejected.length > 0) {
    throw new Error(
      `updater archive contains links or special entries: ${rejected.join(', ')}`,
    );
  }
  return entries;
}

function verifyUpdaterSignature(
  archivePath: string,
  signatureContent: string,
  publicKeyContent: string,
): void {
  const publicLines = decodeWrappedMinisign(
    publicKeyContent,
    'updater public key',
  );
  const signatureLines = decodeWrappedMinisign(
    signatureContent,
    'updater signature',
  );
  if (
    publicLines.length !== 2 ||
    !publicLines[0]?.startsWith('untrusted comment: ')
  ) {
    throw new Error('updater public key has invalid minisign content');
  }
  if (
    signatureLines.length !== 4 ||
    !signatureLines[0]?.startsWith('untrusted comment: ') ||
    !signatureLines[2]?.startsWith('trusted comment: ')
  ) {
    throw new Error('updater signature has invalid minisign content');
  }

  const publicRecord = Buffer.from(publicLines[1] ?? '', 'base64');
  const signatureRecord = Buffer.from(signatureLines[1] ?? '', 'base64');
  const globalSignature = Buffer.from(signatureLines[3] ?? '', 'base64');
  if (
    publicRecord.length !== 42 ||
    signatureRecord.length !== 74 ||
    globalSignature.length !== 64
  ) {
    throw new Error('updater signing data has invalid length');
  }
  const publicAlgorithm = publicRecord.subarray(0, 2).toString('ascii');
  if (
    !['Ed', 'ED'].includes(publicAlgorithm) ||
    !signatureRecord.subarray(0, 2).equals(Buffer.from('ED'))
  ) {
    throw new Error('updater signature must use minisign prehashed mode');
  }
  if (!publicRecord.subarray(2, 10).equals(signatureRecord.subarray(2, 10))) {
    throw new Error('updater signature key does not match the public key');
  }

  const publicKey = createPublicKey({
    key: Buffer.concat([
      Buffer.from('302a300506032b6570032100', 'hex'),
      publicRecord.subarray(10),
    ]),
    format: 'der',
    type: 'spki',
  });
  const digest = createHash('blake2b512')
    .update(readFileSync(archivePath))
    .digest();
  const artifactSignature = signatureRecord.subarray(10);
  if (!verify(null, digest, publicKey, artifactSignature)) {
    throw new Error(`invalid updater signature for ${basename(archivePath)}`);
  }

  const trustedComment = Buffer.from((signatureLines[2] ?? '').slice(17));
  if (
    !verify(
      null,
      Buffer.concat([artifactSignature, trustedComment]),
      publicKey,
      globalSignature,
    )
  ) {
    throw new Error('invalid updater trusted-comment signature');
  }
}

export {
  createUpdateManifest,
  releaseArtifactNames,
  updaterPublicKeyFingerprint,
  validateReleaseAssets,
  validateUpdaterArchiveListing,
  verifyUpdaterSignature,
  writeUpdateManifest,
};

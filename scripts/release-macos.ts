#!/usr/bin/env bun

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import hasExpectedVolumeEntries from './release-layout';
import {
  releaseArtifactNames,
  updaterPublicKeyFingerprint,
  validateReleaseAssets,
  validateUpdaterArchiveListing,
  verifyUpdaterSignature,
  writeUpdateManifest,
} from './updater-release';

interface CommandResult {
  status: number;
  stdout: string;
  stderr: string;
}

interface TauriConfig {
  productName: string;
  version: string;
  bundle?: {
    macOS?: {
      minimumSystemVersion?: string;
    };
  };
}

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const rustupRustc = spawnSync('rustup', ['which', 'rustc'], {
  encoding: 'utf8',
}).stdout?.trim();
const releaseEnvironment: NodeJS.ProcessEnv = {
  ...process.env,
  PATH: rustupRustc
    ? `${dirname(rustupRustc)}:${process.env.PATH ?? ''}`
    : process.env.PATH,
};
const frontendEnvironment: NodeJS.ProcessEnv = Object.fromEntries(
  Object.entries(releaseEnvironment).filter(
    ([key]) =>
      !key.startsWith('APPLE_') &&
      key !== 'TAURI_SIGNING_PRIVATE_KEY' &&
      key !== 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD' &&
      key !== 'TAU_UPDATER_PUBLIC_KEY',
  ),
);
const tauriConfig = JSON.parse(
  readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'),
) as TauriConfig;
const minimumSystemVersion = tauriConfig.bundle?.macOS?.minimumSystemVersion;
const verifyOnly = process.argv[2] === '--verify-only';
const requestedAssets = verifyOnly ? process.argv[3] : undefined;
const targetBundle = join(
  root,
  'src-tauri/target/aarch64-apple-darwin/release/bundle',
);
const releaseAssetsRoot = join(root, 'src-tauri/target/release-assets');
const defaultReleaseAssets = join(releaseAssetsRoot, tauriConfig.version);
const updaterPublicKeyFingerprintPath = join(
  root,
  'scripts/updater-public-key.sha256',
);

class ReleaseError extends Error {}

function fail(message: string): never {
  throw new ReleaseError(message);
}

function command(
  executable: string,
  args: string[],
  options: {
    capture?: boolean;
    allowFailure?: boolean;
    displayCommand?: string;
    env?: NodeJS.ProcessEnv;
  } = {},
): CommandResult {
  const capture = options.capture ?? false;
  const result = spawnSync(executable, args, {
    cwd: root,
    encoding: 'utf8',
    env: options.env ?? releaseEnvironment,
    stdio: capture ? 'pipe' : 'inherit',
  });
  if (result.error)
    fail(`could not run ${executable}: ${result.error.message}`);
  if (result.status !== 0 && !options.allowFailure) {
    const detail = [result.stdout, result.stderr]
      .filter(Boolean)
      .join('\n')
      .trim();
    const displayCommand =
      options.displayCommand ?? [executable, ...args].join(' ');
    fail(`${displayCommand} failed${detail ? `:\n${detail}` : ''}`);
  }
  return {
    status: result.status ?? 1,
    stdout: result.stdout ?? '',
    stderr: result.stderr ?? '',
  };
}

function requireReleaseToolchain(): void {
  const target = 'aarch64-apple-darwin';
  const libdir = command(
    'rustc',
    ['--print', 'target-libdir', '--target', target],
    { capture: true },
  ).stdout.trim();
  if (!libdir || !existsSync(libdir)) {
    fail(
      `the active Rust toolchain does not include ${target}; install it with rustup and ensure its cargo/rustc are first on PATH`,
    );
  }
}

function requirePinnedUpdaterPublicKey(): string {
  const publicKey = process.env.TAU_UPDATER_PUBLIC_KEY?.trim();
  if (!publicKey)
    fail('TAU_UPDATER_PUBLIC_KEY is required as the app updater public key');
  if (!existsSync(updaterPublicKeyFingerprintPath)) {
    fail('the checked-in updater public-key fingerprint is missing');
  }
  const expected = readFileSync(updaterPublicKeyFingerprintPath, 'utf8').trim();
  if (!/^[a-f0-9]{64}$/.test(expected)) {
    fail(
      'the checked-in updater public-key fingerprint is not configured; an authorized owner must pin the production key before release',
    );
  }
  const actual = updaterPublicKeyFingerprint(publicKey);
  if (actual !== expected) {
    fail(
      `TAU_UPDATER_PUBLIC_KEY fingerprint ${actual} does not match the checked-in trust root`,
    );
  }
  return publicKey;
}

function requireSigningCredentials(): void {
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY?.trim()) {
    fail('TAURI_SIGNING_PRIVATE_KEY is required for updater artifacts');
  }
  requirePinnedUpdaterPublicKey();

  const identities = command(
    '/usr/bin/security',
    ['find-identity', '-v', '-p', 'codesigning'],
    { capture: true },
  );
  const identityOutput = `${identities.stdout}\n${identities.stderr}`;
  const developerIds = identityOutput.split('\n').flatMap((line) => {
    const name = line.match(/"([^"]*Developer ID Application:[^"]+)"/)?.[1];
    return name ? [name] : [];
  });
  const importsCertificate = Boolean(
    process.env.APPLE_CERTIFICATE && process.env.APPLE_CERTIFICATE_PASSWORD,
  );
  const requestedIdentity = process.env.APPLE_SIGNING_IDENTITY?.trim();
  if (!importsCertificate && developerIds.length === 0) {
    fail(
      'no Developer ID Application certificate is available; an Apple Development certificate cannot sign a public download',
    );
  }
  if (
    requestedIdentity &&
    !importsCertificate &&
    !developerIds.some(
      (identity) =>
        identity === requestedIdentity ||
        identityOutput.includes(`${requestedIdentity} "${identity}"`),
    )
  ) {
    fail('APPLE_SIGNING_IDENTITY is not a Developer ID Application identity');
  }
  if (!requestedIdentity && !importsCertificate) {
    if (developerIds.length !== 1) {
      fail(
        'multiple Developer ID Application certificates are available; set APPLE_SIGNING_IDENTITY',
      );
    }
    releaseEnvironment.APPLE_SIGNING_IDENTITY = developerIds[0];
  }

  const hasApiCredentials = Boolean(
    process.env.APPLE_API_KEY &&
    process.env.APPLE_API_ISSUER &&
    process.env.APPLE_API_KEY_PATH,
  );
  const hasAppleIdCredentials = Boolean(
    process.env.APPLE_ID &&
    process.env.APPLE_PASSWORD &&
    process.env.APPLE_TEAM_ID,
  );
  if (!hasApiCredentials && !hasAppleIdCredentials) {
    fail(
      'notarization credentials are missing; set APPLE_API_KEY, APPLE_API_ISSUER, and APPLE_API_KEY_PATH, or APPLE_ID, APPLE_PASSWORD, and APPLE_TEAM_ID',
    );
  }
  if (
    hasApiCredentials &&
    !existsSync(resolve(process.env.APPLE_API_KEY_PATH as string))
  ) {
    fail('APPLE_API_KEY_PATH does not exist');
  }
}

function notarizeDmg(dmg: string): void {
  const existingTicket = command(
    '/usr/bin/xcrun',
    ['stapler', 'validate', dmg],
    { allowFailure: true, capture: true },
  );
  if (existingTicket.status === 0) return;

  const args = ['notarytool', 'submit', dmg, '--wait'];
  if (
    process.env.APPLE_API_KEY &&
    process.env.APPLE_API_ISSUER &&
    process.env.APPLE_API_KEY_PATH
  ) {
    args.push(
      '--key',
      resolve(process.env.APPLE_API_KEY_PATH),
      '--key-id',
      process.env.APPLE_API_KEY,
      '--issuer',
      process.env.APPLE_API_ISSUER,
    );
  } else {
    const appleId = process.env.APPLE_ID;
    const teamId = process.env.APPLE_TEAM_ID;
    let password = process.env.APPLE_PASSWORD;
    if (!appleId || !teamId || !password) {
      fail('notarization credentials are missing for the final DMG');
    }
    if (password.startsWith('@env:')) {
      const variable = password.slice('@env:'.length);
      password = process.env[variable];
      if (!password) fail(`the ${variable} environment variable is empty`);
    } else if (password.startsWith('@keychain:')) {
      const item = password.slice('@keychain:'.length);
      password = command(
        '/usr/bin/security',
        ['find-generic-password', '-w', '-s', item],
        { capture: true },
      ).stdout.trim();
    }
    args.push(
      '--apple-id',
      appleId,
      '--password',
      password,
      '--team-id',
      teamId,
    );
  }
  command('/usr/bin/xcrun', args, {
    displayCommand: 'xcrun notarytool submit <DMG> --wait',
  });
  command('/usr/bin/xcrun', ['stapler', 'staple', dmg]);
}

function requireSynchronizedVersions(): void {
  const packageVersion = (
    JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
      version: string;
    }
  ).version;
  const cargo = readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf8');
  const cargoVersion = cargo.match(/^version = "([^"]+)"$/m)?.[1];
  const versions = [packageVersion, tauriConfig.version, cargoVersion];
  if (versions.some((version) => version !== versions[0])) {
    fail(`release versions disagree: ${versions.join(', ')}`);
  }
}

function onlyArtifact(directory: string, suffix: string): string {
  if (!existsSync(directory))
    fail(`bundle output directory is missing: ${directory}`);
  const candidates = readdirSync(directory).filter((name) =>
    name.endsWith(suffix),
  );
  if (candidates.length !== 1) {
    fail(
      `expected exactly one ${suffix} in ${directory}, found ${candidates.join(', ') || 'none'}`,
    );
  }
  return join(directory, candidates[0] as string);
}

function requireText(value: string, expected: string, subject: string): void {
  if (!value.includes(expected))
    fail(`${subject} does not include ${expected}`);
}

function appBundleFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isSymbolicLink() || lstatSync(path).isSymbolicLink()) {
      fail(`the app bundle contains a symbolic link: ${path}`);
    }
    if (entry.isDirectory()) return appBundleFiles(path);
    return entry.isFile() ? [path] : [];
  });
}

function appContentHashes(app: string): Map<string, string> {
  return new Map(
    appBundleFiles(app)
      .sort((left, right) =>
        relative(app, left).localeCompare(relative(app, right)),
      )
      .map((path) => [
        relative(app, path),
        createHash('sha256').update(readFileSync(path)).digest('hex'),
      ]),
  );
}

function verifySignature(path: string, architecture?: string): void {
  const args = ['-dv', '--verbose=4'];
  if (architecture) args.push('--arch', architecture);
  args.push(path);
  const signature = command('/usr/bin/codesign', args, { capture: true });
  const details = `${signature.stdout}\n${signature.stderr}`;
  const subject = architecture ? `${architecture} signature` : 'app signature';
  requireText(details, 'Authority=Developer ID Application:', subject);
  requireText(details, 'TeamIdentifier=', subject);
  if (!/flags=.*\(.*\bruntime\b.*\)/.test(details)) {
    fail(`the ${subject} does not enable the hardened runtime`);
  }
}

function verifyDynamicLibraries(binary: string, architecture: string): void {
  const output = command(
    '/usr/bin/otool',
    ['-L', '-arch', architecture, binary],
    { capture: true },
  ).stdout;
  const unexpected = output
    .split('\n')
    .slice(1)
    .flatMap((line) => {
      const path = line.trim().split(' ')[0];
      return path ? [path] : [];
    })
    .filter(
      (path) =>
        !path.startsWith('/System/Library/') && !path.startsWith('/usr/lib/'),
    );
  if (unexpected.length > 0) {
    fail(`the ${architecture} executable uses local libraries: ${unexpected}`);
  }
}

function verifyStandaloneApp(app: string): Map<string, string> {
  if (!minimumSystemVersion) {
    fail('bundle.macOS.minimumSystemVersion is not configured');
  }
  const binary = join(app, 'Contents/MacOS/tau');
  if (!existsSync(binary))
    fail('the archive does not contain the Tau executable');
  const files = appBundleFiles(app);
  const appFiles = files.map((path) => relative(app, path)).sort();
  const expectedAppFiles = [
    'Contents/CodeResources',
    'Contents/Info.plist',
    'Contents/MacOS/tau',
    'Contents/Resources/icon.icns',
    'Contents/_CodeSignature/CodeResources',
  ].sort();
  if (appFiles.join(',') !== expectedAppFiles.join(',')) {
    fail(`the updater app bundle contains unexpected files: ${appFiles}`);
  }
  const machOFiles = files.filter((path) =>
    command('/usr/bin/file', ['-b', path], { capture: true }).stdout.includes(
      'Mach-O',
    ),
  );
  if (machOFiles.length !== 1 || machOFiles[0] !== binary) {
    fail(
      `the updater app bundle contains unexpected executable code: ${machOFiles}`,
    );
  }
  const architectures = command('/usr/bin/lipo', ['-archs', binary], {
    capture: true,
  })
    .stdout.trim()
    .split(/\s+/)
    .sort();
  if (architectures.join(',') !== 'arm64') {
    fail(`the release executable architectures are ${architectures}`);
  }

  command('/usr/bin/codesign', [
    '--verify',
    '--deep',
    '--strict',
    '--verbose=2',
    app,
  ]);
  verifySignature(app);
  command('/usr/sbin/spctl', [
    '--assess',
    '--type',
    'execute',
    '--verbose=2',
    app,
  ]);
  command('/usr/bin/xcrun', ['stapler', 'validate', app]);

  const declaredMinimum = command(
    '/usr/libexec/PlistBuddy',
    ['-c', 'Print :LSMinimumSystemVersion', join(app, 'Contents/Info.plist')],
    { capture: true },
  ).stdout.trim();
  if (declaredMinimum !== minimumSystemVersion) {
    fail(
      `Info.plist requires macOS ${declaredMinimum}, expected ${minimumSystemVersion}`,
    );
  }
  verifySignature(app, 'arm64');
  const loadCommands = command(
    '/usr/bin/otool',
    ['-l', '-arch', 'arm64', binary],
    { capture: true },
  ).stdout;
  const deploymentTargets = [
    ...loadCommands.matchAll(/^\s+minos (\S+)$/gm),
  ].map((match) => match[1]);
  if (
    deploymentTargets.length === 0 ||
    deploymentTargets.some((target) => target !== minimumSystemVersion)
  ) {
    fail(
      `the arm64 deployment target is ${deploymentTargets.join(', ') || 'missing'}, expected ${minimumSystemVersion}`,
    );
  }
  const rpaths = [
    ...loadCommands.matchAll(/^\s+path (\S+) \(offset \d+\)$/gm),
  ].map((match) => match[1]);
  const localRpaths = rpaths.filter(
    (path) =>
      path &&
      !path.startsWith('/System/Library/') &&
      !path.startsWith('/usr/lib/') &&
      !path.startsWith('@executable_path/') &&
      !path.startsWith('@loader_path/'),
  );
  if (localRpaths.length > 0) {
    fail(`the arm64 executable uses local rpaths: ${localRpaths}`);
  }
  verifyDynamicLibraries(binary, 'arm64');
  return appContentHashes(app);
}

function verifyDmg(dmg: string, expectedAppHashes?: Map<string, string>): void {
  if (!existsSync(dmg)) fail(`DMG does not exist: ${dmg}`);
  if (!minimumSystemVersion) {
    fail('bundle.macOS.minimumSystemVersion is not configured');
  }

  command('/usr/bin/codesign', ['--verify', '--verbose=2', dmg]);
  command('/usr/sbin/spctl', [
    '--assess',
    '--type',
    'open',
    '--context',
    'context:primary-signature',
    '--verbose=2',
    dmg,
  ]);
  command('/usr/bin/xcrun', ['stapler', 'validate', dmg]);

  const temporary = mkdtempSync(join(tmpdir(), 'tau-release-'));
  const mount = join(temporary, 'mount');
  mkdirSync(mount);
  let mounted = false;
  let verificationFailed = false;
  try {
    command('/usr/bin/hdiutil', [
      'attach',
      '-quiet',
      '-nobrowse',
      '-readonly',
      '-mountpoint',
      mount,
      dmg,
    ]);
    mounted = true;
    const volumeEntries = readdirSync(mount).sort();
    if (!hasExpectedVolumeEntries(volumeEntries, tauriConfig.productName)) {
      fail(`the DMG contains unexpected files: ${volumeEntries}`);
    }
    const applicationsLink = join(mount, 'Applications');
    if (
      !lstatSync(applicationsLink).isSymbolicLink() ||
      readlinkSync(applicationsLink) !== '/Applications'
    ) {
      fail('the DMG Applications link is invalid');
    }

    const app = join(mount, `${tauriConfig.productName}.app`);
    if (expectedAppHashes) {
      const actualAppHashes = appContentHashes(app);
      if (
        JSON.stringify([...actualAppHashes]) !==
        JSON.stringify([...expectedAppHashes])
      ) {
        fail('the finalized app in the DMG differs from the updater archive');
      }
    }
    const binary = join(app, 'Contents/MacOS/tau');
    if (!existsSync(binary))
      fail('the DMG does not contain the Tau executable');
    const files = appBundleFiles(app);
    const appFiles = files.map((path) => relative(app, path)).sort();
    const expectedAppFiles = [
      'Contents/CodeResources',
      'Contents/Info.plist',
      'Contents/MacOS/tau',
      'Contents/Resources/icon.icns',
      'Contents/_CodeSignature/CodeResources',
    ].sort();
    if (appFiles.join(',') !== expectedAppFiles.join(',')) {
      fail(`the app bundle contains unexpected files: ${appFiles}`);
    }
    const machOFiles = files.filter((path) =>
      command('/usr/bin/file', ['-b', path], { capture: true }).stdout.includes(
        'Mach-O',
      ),
    );
    if (machOFiles.length !== 1 || machOFiles[0] !== binary) {
      fail(`the app bundle contains unexpected executable code: ${machOFiles}`);
    }

    const architectures = command('/usr/bin/lipo', ['-archs', binary], {
      capture: true,
    })
      .stdout.trim()
      .split(/\s+/)
      .sort();
    if (architectures.join(',') !== 'arm64') {
      fail(`the release executable architectures are ${architectures}`);
    }

    command('/usr/bin/codesign', [
      '--verify',
      '--deep',
      '--strict',
      '--verbose=2',
      app,
    ]);
    verifySignature(app);

    command('/usr/sbin/spctl', [
      '--assess',
      '--type',
      'execute',
      '--verbose=2',
      app,
    ]);
    command('/usr/bin/xcrun', ['stapler', 'validate', app]);

    const declaredMinimum = command(
      '/usr/libexec/PlistBuddy',
      ['-c', 'Print :LSMinimumSystemVersion', join(app, 'Contents/Info.plist')],
      { capture: true },
    ).stdout.trim();
    if (declaredMinimum !== minimumSystemVersion) {
      fail(
        `Info.plist requires macOS ${declaredMinimum}, expected ${minimumSystemVersion}`,
      );
    }

    for (const architecture of ['arm64']) {
      verifySignature(app, architecture);
      const loadCommands = command(
        '/usr/bin/otool',
        ['-l', '-arch', architecture, binary],
        { capture: true },
      ).stdout;
      const deploymentTargets = [
        ...loadCommands.matchAll(/^\s+minos (\S+)$/gm),
      ].map((match) => match[1]);
      if (
        deploymentTargets.length === 0 ||
        deploymentTargets.some((target) => target !== minimumSystemVersion)
      ) {
        fail(
          `the ${architecture} deployment target is ${deploymentTargets.join(', ') || 'missing'}, expected ${minimumSystemVersion}`,
        );
      }
      const rpaths = [
        ...loadCommands.matchAll(/^\s+path (\S+) \(offset \d+\)$/gm),
      ].map((match) => match[1]);
      const localRpaths = rpaths.filter(
        (path) =>
          path &&
          !path.startsWith('/System/Library/') &&
          !path.startsWith('/usr/lib/') &&
          !path.startsWith('@executable_path/') &&
          !path.startsWith('@loader_path/'),
      );
      if (localRpaths.length > 0) {
        fail(
          `the ${architecture} executable uses local rpaths: ${localRpaths}`,
        );
      }
      verifyDynamicLibraries(binary, architecture);
    }
  } catch (error) {
    verificationFailed = true;
    throw error;
  } finally {
    let detached = true;
    if (mounted) {
      const result = command('/usr/bin/hdiutil', ['detach', '-quiet', mount], {
        allowFailure: true,
        capture: true,
      });
      detached = result.status === 0;
      if (!detached && verificationFailed) {
        console.error(`macOS release: additionally, could not detach ${mount}`);
      }
    }
    if (detached) {
      try {
        rmSync(temporary, { recursive: true, force: true });
      } catch {
        if (verificationFailed) {
          console.error(
            `macOS release: additionally, could not remove ${temporary}`,
          );
        } else {
          fail(`could not remove ${temporary}`);
        }
      }
    } else if (!verificationFailed) {
      fail(`could not detach ${mount}`);
    }
  }

  const checksum = command('/usr/bin/shasum', ['-a', '256', dmg], {
    capture: true,
  }).stdout.trim();
  console.log(`Verified ${basename(dmg)}\n${checksum}`);
}

function requireReleaseDestinationAvailable(): void {
  if (existsSync(defaultReleaseAssets)) {
    fail(
      `release destination already exists: ${defaultReleaseAssets}; preserve or remove it explicitly before rebuilding`,
    );
  }
}

function stageReleaseAssets(
  dmg: string,
  archive: string,
  signature: string,
): string {
  const names = releaseArtifactNames(tauriConfig.version);
  mkdirSync(releaseAssetsRoot, { recursive: true });
  requireReleaseDestinationAvailable();
  const temporary = mkdtempSync(
    join(releaseAssetsRoot, `.${tauriConfig.version}-staging-`),
  );
  let committed = false;
  try {
    copyFileSync(dmg, join(temporary, names.dmg));
    copyFileSync(archive, join(temporary, names.archive));
    copyFileSync(signature, join(temporary, names.signature));
    writeUpdateManifest(temporary, tauriConfig.version);
    validateReleaseAssets(temporary, tauriConfig.version);
    renameSync(temporary, defaultReleaseAssets);
    committed = true;
    return defaultReleaseAssets;
  } finally {
    if (!committed) rmSync(temporary, { recursive: true, force: true });
  }
}

function verifyReleaseAssets(directory: string): void {
  const manifest = validateReleaseAssets(directory, tauriConfig.version);
  const names = releaseArtifactNames(tauriConfig.version);
  const archive = join(directory, names.archive);
  const signature = readFileSync(join(directory, names.signature), 'utf8');
  const publicKey = requirePinnedUpdaterPublicKey();
  if (manifest.platforms['darwin-aarch64'].signature !== signature) {
    fail('latest.json signature does not exactly match the signature asset');
  }
  verifyUpdaterSignature(archive, signature, publicKey);

  const pathListing = command('/usr/bin/tar', ['-tzf', archive], {
    capture: true,
  }).stdout;
  const verboseListing = command('/usr/bin/tar', ['-tvzf', archive], {
    capture: true,
  }).stdout;
  const entries = validateUpdaterArchiveListing(pathListing, verboseListing);
  const appName = `${tauriConfig.productName}.app`;
  if (
    entries.length === 0 ||
    entries.some((entry) => {
      const normalized = entry.replace(/^\.\//, '');
      return (
        (normalized !== appName && !normalized.startsWith(`${appName}/`)) ||
        normalized.split('/').includes('..')
      );
    })
  ) {
    fail(`the updater archive contains unexpected paths: ${entries}`);
  }

  const temporary = mkdtempSync(join(tmpdir(), 'tau-updater-'));
  try {
    command('/usr/bin/tar', ['-xzf', archive, '-C', temporary]);
    const hashes = verifyStandaloneApp(join(temporary, appName));
    verifyDmg(join(directory, names.dmg), hashes);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }

  for (const name of Object.values(names)) {
    const checksum = command(
      '/usr/bin/shasum',
      ['-a', '256', join(directory, name)],
      { capture: true },
    ).stdout.trim();
    console.log(checksum);
  }
}

function buildRelease(): string {
  command('bun', ['run', 'build'], { env: frontendEnvironment });
  const publicKey = requirePinnedUpdaterPublicKey();
  requireReleaseDestinationAvailable();
  const releaseConfig = JSON.parse(
    readFileSync(join(root, 'src-tauri/tauri.release.conf.json'), 'utf8'),
  ) as Record<string, unknown>;
  const temporary = mkdtempSync(join(tmpdir(), 'tau-release-config-'));
  const configPath = join(temporary, 'tauri.release.conf.json');
  writeFileSync(
    configPath,
    JSON.stringify({
      ...releaseConfig,
      plugins: { updater: { pubkey: publicKey } },
    }),
  );

  rmSync(targetBundle, { recursive: true, force: true });
  try {
    command('bun', [
      'tauri',
      'build',
      '--target',
      'aarch64-apple-darwin',
      '--bundles',
      'app,dmg',
      '--config',
      configPath,
      '--ci',
    ]);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }

  const dmg = onlyArtifact(join(targetBundle, 'dmg'), '.dmg');
  const archive = onlyArtifact(join(targetBundle, 'macos'), '.app.tar.gz');
  const signature = onlyArtifact(
    join(targetBundle, 'macos'),
    '.app.tar.gz.sig',
  );
  notarizeDmg(dmg);
  return stageReleaseAssets(dmg, archive, signature);
}

function main(): void {
  if (process.platform !== 'darwin')
    fail('releases must be built and verified on macOS');
  requireSynchronizedVersions();

  if (verifyOnly) {
    verifyReleaseAssets(
      requestedAssets ? resolve(requestedAssets) : defaultReleaseAssets,
    );
    return;
  }

  requireReleaseToolchain();
  requireSigningCredentials();
  verifyReleaseAssets(buildRelease());
}

try {
  main();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`macOS release: ${message}`);
  process.exitCode = 1;
}

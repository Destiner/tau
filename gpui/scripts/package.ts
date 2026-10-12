import {
  chmodSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { homedir } from 'node:os';
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
} from 'node:path';

const root = resolve(import.meta.dir, '..');
export const bundleId = 'com.destiner.tau.native-preview';
export type Runner = (args: string[]) => string;
const run: Runner = (args) => {
  console.log(args.join(' '));
  const result = Bun.spawnSync(args, {
    cwd: root,
    env: { ...process.env, CARGO_BUILD_JOBS: '2' },
    stdout: 'pipe',
    stderr: 'pipe',
  });
  if (result.exitCode !== 0)
    throw new Error(`${args[0]} failed: ${result.stderr.toString()}`);
  return result.stdout.toString();
};

export function plan(args: string[]): {
  output: string;
  debug: boolean;
  noBuild: boolean;
  binary: string;
  build: string[];
} {
  let debug = false,
    noBuild = false;
  let output = join(root, 'target/bundle/macos/Tau.app');
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--debug') debug = true;
    else if (args[i] === '--no-build') noBuild = true;
    else if (args[i] === '--output') {
      const value = args[++i];
      if (!value || !isAbsolute(value))
        throw new Error('--output requires an absolute path');
      output = resolve(value);
    } else throw new Error(`Unknown option: ${args[i]}`);
  }
  if (basename(output) !== 'Tau.app')
    throw new Error('Output must be named Tau.app');
  assertSafeLocation(output);
  return {
    output,
    debug,
    noBuild,
    binary: join(root, 'target', debug ? 'debug' : 'release', 'tau'),
    build: [
      'cargo',
      'build',
      '--locked',
      '--manifest-path',
      join(root, 'Cargo.toml'),
      '--bin',
      'tau',
      '--target-dir',
      join(root, 'target'),
      ...(debug ? [] : ['--release']),
    ],
  };
}

export function plist(version: string, debug: boolean): string {
  if (!/^\d+\.\d+\.\d+$/.test(version))
    throw new Error('Expected a numeric Cargo package version');
  const values = {
    CFBundleIdentifier: bundleId,
    CFBundleName: 'Tau',
    CFBundleDisplayName: 'Tau',
    CFBundleExecutable: 'tau',
    CFBundlePackageType: 'APPL',
    CFBundleIconFile: 'icon.icns',
    CFBundleShortVersionString: version,
    CFBundleVersion: version,
    CFBundleInfoDictionaryVersion: '6.0',
    LSMinimumSystemVersion: '15.0',
    NSHumanReadableCopyright: `Tau ${version} — GPUI ${debug ? 'debug' : 'release'} preview; local ad-hoc build`,
    NSHighResolutionCapable: true,
  };
  return `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0"><dict>\n${Object.entries(
    values,
  )
    .map(
      ([key, value]) =>
        `<key>${key}</key>${value === true ? '<true/>' : `<string>${value}</string>`}`,
    )
    .join('\n')}\n</dict></plist>\n`;
}

export function verifyBinary(binary: string, exec: Runner): string {
  const file = exec(['/usr/bin/file', '-b', binary]);
  if (!file.includes('Mach-O') || !file.includes('executable'))
    throw new Error('Input is not a Mach-O executable');
  const arch = exec(['/usr/bin/lipo', '-archs', binary]).trim();
  if (!/^(arm64|x86_64)( (arm64|x86_64))?$/.test(arch))
    throw new Error(`Unsupported architecture: ${arch}`);
  const libraries = exec(['/usr/bin/otool', '-L', binary])
    .trim()
    .split('\n')
    .slice(1);
  if (
    !libraries.length ||
    libraries.some(
      (line) => !/^\s+(\/System\/Library\/|\/usr\/lib\/)/.test(line),
    )
  )
    throw new Error(
      'Non-system dynamic library dependency; cannot package portably',
    );
  const commands = exec(['/usr/bin/otool', '-l', binary]);
  for (const match of commands.matchAll(
    /cmd LC_RPATH\s+cmdsize \d+\s+path (.*?) \(offset/g,
  )) {
    if (!/^\/(System\/Library|usr\/lib)\//.test(match[1]!))
      throw new Error(`Non-system rpath: ${match[1]}`);
  }
  return arch;
}

function canonicalDestination(path: string): string {
  let ancestor = resolve(path);
  const suffix: string[] = [];
  for (;;) {
    try {
      return resolve(realpathSync(ancestor), ...suffix);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
      lstatIfPresent(ancestor);
      const parent = dirname(ancestor);
      if (parent === ancestor) throw error;
      suffix.unshift(basename(ancestor));
      ancestor = parent;
    }
  }
}

function lstatIfPresent(path: string): void {
  try {
    lstatSync(path);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return;
    throw error;
  }
  throw new Error(`Unresolved destination symlink: ${path}`);
}

function assertSafeLocation(output: string): void {
  const locations = [
    '/Applications',
    '/System/Applications',
    join(homedir(), 'Applications'),
  ];
  const paths = [resolve(output), canonicalDestination(output)];
  for (const location of locations) {
    for (const base of [resolve(location), canonicalDestination(location)]) {
      if (
        paths.some((path) => {
          const child = relative(base, path);
          return (
            child === '' ||
            (!child.startsWith('../') && child !== '..' && !isAbsolute(child))
          );
        })
      )
        throw new Error(
          'Applications install locations are forbidden for previews',
        );
    }
  }
}

// Fresh destinations only: even a marked bundle is never recursively replaced.
export function assertFresh(output: string): void {
  try {
    lstatSync(output);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      assertSafeLocation(output);
      return;
    }
    throw error;
  }
  throw new Error(
    `Output already exists; choose a fresh --output directory: ${output}`,
  );
}

export function packageApp(args: string[], exec: Runner = run): string {
  if (process.platform !== 'darwin')
    throw new Error('macOS packaging requires macOS');
  const options = plan(args);
  assertFresh(options.output);
  if (!options.noBuild) exec(options.build);
  if (!existsSync(options.binary) || !lstatSync(options.binary).isFile())
    throw new Error(`Missing regular binary: ${options.binary}`);
  verifyBinary(options.binary, exec);
  const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8');
  const version = cargo
    .split('[package]')[1]
    ?.split(/\n\[/)[0]
    ?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) throw new Error('Missing Cargo package version');
  const metadata = plist(version, options.debug);
  mkdirSync(dirname(options.output), { recursive: true });
  const stage = mkdtempSync(
    join(dirname(options.output), '.tau-native-package-'),
  );
  const app = join(stage, 'Tau.app');
  try {
    mkdirSync(join(app, 'Contents/MacOS'), { recursive: true });
    mkdirSync(join(app, 'Contents/Resources'));
    copyFileSync(options.binary, join(app, 'Contents/MacOS/tau'));
    chmodSync(join(app, 'Contents/MacOS/tau'), 0o755);
    copyFileSync(
      join(root, '../src-tauri/icons/icon.icns'),
      join(app, 'Contents/Resources/icon.icns'),
    );
    writeFileSync(join(app, 'Contents/Info.plist'), metadata);
    writeFileSync(
      join(app, 'Contents/Resources/tau-native-preview.json'),
      JSON.stringify({
        owner: 'gpui/scripts/package.ts',
        version,
        profile: options.debug ? 'debug' : 'release',
      }),
    );
    exec(['/usr/bin/plutil', '-lint', join(app, 'Contents/Info.plist')]);
    exec(['/usr/bin/codesign', '--force', '--sign', '-', app]);
    exec([
      '/usr/bin/codesign',
      '--verify',
      '--deep',
      '--strict',
      '--verbose=2',
      app,
    ]);
    const arch = verifyBinary(join(app, 'Contents/MacOS/tau'), exec);
    assertFresh(options.output);
    renameSync(app, options.output);
    console.log(
      `Packaged ${options.output}\n${arch}; ad-hoc signed GPUI preview ${version}; not notarized. No launch performed.`,
    );
    return options.output;
  } finally {
    rmSync(stage, { recursive: true, force: true });
  }
}

if (import.meta.main) {
  try {
    packageApp(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : 'Packaging failed');
    process.exitCode = 1;
  }
}

import { createHash } from 'node:crypto';
import { readFile, readdir } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dir, '../..');
export type CaptureProvenance = {
  runId: string;
  browser: 'chromium' | 'webkit';
  renderInputs: string;
  nativeBinary: string;
  referenceRunId?: string;
  complete: boolean;
  artifacts?: string;
};

export async function hashPaths(
  base: string,
  paths: string[],
): Promise<string> {
  const files: string[] = [];
  async function collect(path: string): Promise<void> {
    for (const entry of await readdir(resolve(base, path), {
      withFileTypes: true,
    })) {
      const child = `${path}/${entry.name}`;
      if (entry.isDirectory()) await collect(child);
      else if (entry.isFile() && !entry.name.endsWith('.md')) files.push(child);
      else if (entry.isSymbolicLink())
        throw new Error(`Fingerprint cannot follow symlink: ${child}`);
    }
  }
  for (const path of paths) {
    // Paths ending in / explicitly designate a source directory.
    if (path.endsWith('/')) await collect(path.slice(0, -1));
    else files.push(path);
  }
  const hash = createHash('sha256');
  for (const path of [...new Set(files)].sort())
    hash
      .update(path)
      .update('\0')
      .update(await readFile(resolve(base, path)));
  return hash.digest('hex');
}

export async function renderInputsHash(): Promise<string> {
  const paths = [
    'gpui/contract.ts',
    'gpui/scenarios.ts',
    'gpui/scripts/browser.ts',
    'gpui/Cargo.toml',
    'gpui/Cargo.lock',
    'gpui/bun.lock',
    'src/components/ui/UiButton.vue',
    'src/components/ui/UiInput.vue',
    'src/components/ui/UiTextarea.vue',
    'src/styles.css',
    'package.json',
    'bun.lock',
  ];
  return hashPaths(root, [
    ...paths,
    'gpui/src/',
    'gpui/reference/',
    'gpui/assets/',
    'gpui/patches/',
  ]);
}

export async function captureArtifactsHash(folder: string): Promise<string> {
  const paths = ['request.json'];
  for (const directory of ['vue', 'native', 'report']) {
    for (const file of await readdir(resolve(folder, directory))) {
      if (
        file.endsWith('.png') ||
        file === 'observations.json' ||
        file === 'report.json'
      )
        paths.push(`${directory}/${file}`);
    }
  }
  return hashPaths(folder, paths);
}

export async function nativeBinaryHash(
  binary: 'tau-gpui-prototype' | 'transcript-probe' = 'tau-gpui-prototype',
): Promise<string> {
  return createHash('sha256')
    .update(await readFile(resolve(root, 'gpui/target/debug', binary)))
    .digest('hex');
}

export function assertCurrentCapture(
  capture: CaptureProvenance,
  inputs: string,
  binary: string,
): void {
  if (
    !capture.complete ||
    !capture.runId ||
    !binary ||
    capture.renderInputs !== inputs ||
    capture.nativeBinary !== binary
  )
    throw new Error(
      'review requires a completed capture of the current sources and native binary; recapture',
    );
}

export function assertCapturePair(
  chromium: CaptureProvenance,
  webkit: CaptureProvenance,
): void {
  if (
    chromium.browser !== 'chromium' ||
    webkit.browser !== 'webkit' ||
    !chromium.complete ||
    !webkit.complete ||
    !chromium.runId ||
    webkit.referenceRunId !== chromium.runId ||
    !chromium.renderInputs ||
    chromium.renderInputs !== webkit.renderInputs ||
    !chromium.nativeBinary ||
    chromium.nativeBinary !== webkit.nativeBinary
  ) {
    throw new Error(
      'follow-up requires a completed Chromium capture followed by a matching WebKit capture; rerun both browsers',
    );
  }
}

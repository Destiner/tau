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
};

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
  for (const folder of [
    'gpui/src',
    'gpui/reference',
    'gpui/assets',
    'gpui/patches',
  ]) {
    for (const file of await readdir(resolve(root, folder))) {
      if (!file.endsWith('.md')) paths.push(`${folder}/${file}`);
    }
  }
  const hash = createHash('sha256');
  for (const path of paths.sort())
    hash
      .update(path)
      .update('\0')
      .update(await readFile(resolve(root, path)));
  return hash.digest('hex');
}

export async function nativeBinaryHash(): Promise<string> {
  return createHash('sha256')
    .update(
      await readFile(resolve(root, 'gpui/target/debug/tau-gpui-prototype')),
    )
    .digest('hex');
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

import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { validateSuite, type Suite } from '../contract';
import { suite as currentSuite } from '../scenarios';
import type { Row } from './compare';
import { renderReport } from './report';
import {
  assertCapturePair,
  assertCurrentCapture,
  captureArtifactsHash,
  nativeBinaryHash,
  renderInputsHash,
  type CaptureProvenance,
} from './provenance';

export const textareaNotes: Record<string, string> = {
  'textarea-placeholder/idle':
    'Empty bordered multiline field. The native version keeps the already accepted thinner focus treatment.',
  'textarea-blank-lines-and-selection/empty-row':
    'Real caret on the blank second line after ArrowDown, at UTF-16 offset 3. No synthetic cursor overlay.',
  'textarea-blank-lines-and-selection/backward-row':
    'Shift+Up selects backward across the newline while retaining its anchor.',
  'textarea-blank-lines-and-selection/replace-across-newline':
    'Typing replaces the selection across a newline, leaving ab / Xd and one Change event.',
  'textarea-unicode-navigation/past-emoji':
    'Real caret immediately after the emoji. Browser and Base byte offsets are compared through the UTF-16 contract.',
  'textarea-unicode-navigation/selected-grapheme':
    'Shift+Left selects the entire joined emoji on the next line, not part of its code-point sequence.',
  'textarea-wrapped-rows/visual-row-start':
    'Command+Left uses the wrapped visual row, not the entire newline-delimited paragraph.',
  'textarea-prop-retention/resized':
    'Changing width, height and rows preserves the selection and draft without emitting Change.',
  'textarea-prop-retention/disabled':
    'Disabling removes focus and prevents editing while preserving the draft and visible selection.',
  'textarea-narrow-reflow/narrow-row-start':
    'Changing width reflows text; subsequent navigation uses the new visual rows.',
  'textarea-long-scroll/top':
    'Long content at the top of a bounded viewport. Caret is hidden in this scrolling sequence.',
  'textarea-long-scroll/scrolled':
    'Real wheel input scrolls to the bottom without changing value or selection. A pixel regression requires content movement inside the field and exact restoration when scrolling back.',
  'textarea-undo-and-shortcuts/undo':
    'Undo restores the multiline replacement; redo and Select All/collapse are also covered in the full report.',
  'textarea-bare/focused':
    'Bare variant for embedded editing. The live playground also exposes mono, readonly, disabled, width and height controls.',
};

export function textareaRows(rows: Row[]): Row[] {
  return Object.keys(textareaNotes).map((id) => {
    const matches = rows.filter((row) => row.id === id);
    if (matches.length !== 1)
      throw new Error(`textarea review requires exactly one ${id}`);
    return matches[0]!;
  });
}

export async function reviewTextarea(
  base = resolve(import.meta.dir, '../results/multiline'),
): Promise<void> {
  const inputs = await renderInputsHash();
  const binary = await nativeBinaryHash();
  for (const theme of ['light', 'dark'] as const) {
    const provenance = await Promise.all(
      ['chromium', 'webkit'].map(
        async (browser) =>
          JSON.parse(
            await readFile(
              resolve(base, `${theme}-${browser}/capture.json`),
              'utf8',
            ),
          ) as CaptureProvenance,
      ),
    );
    for (const capture of provenance)
      assertCurrentCapture(capture, inputs, binary);
    assertCapturePair(provenance[0]!, provenance[1]!);
    for (const browser of ['chromium', 'webkit'] as const) {
      const folder = resolve(base, `${theme}-${browser}`);
      const capture = provenance[browser === 'chromium' ? 0 : 1]!;
      if (
        !capture.artifacts ||
        capture.artifacts !== (await captureArtifactsHash(folder))
      )
        throw new Error(
          'Changed or unverified multiline report/images; recapture',
        );
      const suite: Suite = JSON.parse(
        await readFile(resolve(folder, 'request.json'), 'utf8'),
      );
      validateSuite(suite);
      if (
        JSON.stringify(suite) !==
        JSON.stringify(currentSuite(theme, 'textarea'))
      )
        throw new Error(
          'textarea review requires the current complete textarea suite',
        );
      const report: { checkpoints: Row[] } = JSON.parse(
        await readFile(resolve(folder, 'report/report.json'), 'utf8'),
      );
      const rows = textareaRows(report.checkpoints);
      const html = renderReport(suite, rows, {
        title: `Multiline input · ${theme} · ${browser === 'webkit' ? 'Vue WebKit ↔ GPUI' : 'Vue Chromium ↔ GPUI'}`,
        intro:
          'Only multiline cases are shown. Use the live native playground to type, select, scroll and change width/height. Native drag-to-resize and OS IME/clipboard are not certified by these captures. The live dimensions are parent-controlled; this is not yet the auto-growing application composer.',
        notes: textareaNotes,
        links: [
          {
            label: 'Light',
            href: `../../light-${browser}/report/textarea.html`,
          },
          { label: 'Dark', href: `../../dark-${browser}/report/textarea.html` },
          {
            label:
              browser === 'webkit' ? 'Chromium reference' : 'WebKit reference',
            href: `../../${theme}-${browser === 'webkit' ? 'chromium' : 'webkit'}/report/textarea.html`,
          },
          { label: 'All multiline checkpoints', href: 'report.html' },
        ],
      });
      await writeFile(resolve(folder, 'report/textarea.html'), html);
      console.log(resolve(folder, 'report/textarea.html'));
    }
  }
}
if (import.meta.main) await reviewTextarea(process.argv[2]);

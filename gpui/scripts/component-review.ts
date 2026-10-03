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

export const textareaFeedbackNotes: Record<string, string> = {
  'textarea-long-scroll/top':
    'Tau now uses half-speed wheel/trackpad scrolling in the multiline input. Real event-and-pixel tests verify that 32px of wheel input moves content 16 logical px, and small fractional deltas accumulate rather than getting dropped. This is a deliberate feel adjustment, not a claim of Vue scroll-speed parity.',
  'textarea-long-scroll/scrolled':
    'The scrollbar thumb now has rounded ends in normal, hover and drag states. A real pointer-drag regression checks the caps and confirms that dragging remains direct, not slowed by the wheel setting.',
  'textarea-placeholder/idle':
    'Placeholder after explicitly disabling CoreGraphics font smoothing. The authored colour and 400 font weight are unchanged.',
  'textarea-multiline/typed':
    'Regular Inter text with corrected grayscale rasterization. New regression probes keep native glyph coverage within 5% of WebKit; the measured difference is currently under 1%.',
  'textarea-wrapped-rows/visual-row-start':
    'Monospaced text now uses the same macOS system monospace family as CSS ui-monospace, rather than its Menlo fallback.',
  'textarea-blank-lines-and-selection/empty-row':
    'The real native caret is now 2 logical px wide (4 physical px at this scale), centered on the insertion boundary. Vue retains its own native caret width.',
  'textarea-blank-lines-and-selection/backward-row':
    'The [3,5] selection is unchanged. Its selected hard-newline row now fills the content width and uses font-metric leading alignment, matching WebKit rather than Chromium’s narrow newline cell.',
};

export function textareaRows(rows: Row[], notes = textareaNotes): Row[] {
  return Object.keys(notes).map((id) => {
    const matches = rows.filter((row) => row.id === id);
    if (matches.length !== 1)
      throw new Error(`textarea review requires exactly one ${id}`);
    return matches[0]!;
  });
}

export async function reviewTextarea(
  base = resolve(import.meta.dir, '../results/multiline'),
  feedback = false,
): Promise<void> {
  const filename = feedback ? 'textarea-feedback.html' : 'textarea.html';
  const notes = feedback ? textareaFeedbackNotes : textareaNotes;
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
      const rows = textareaRows(report.checkpoints, notes);
      const html = renderReport(suite, rows, {
        title: `${feedback ? 'Multiline rendering follow-up' : 'Multiline input'} · ${theme} · ${browser === 'webkit' ? 'Vue WebKit ↔ GPUI' : 'Vue Chromium ↔ GPUI'}`,
        intro:
          'Only multiline cases are shown. Use the live native playground to type, select, scroll and change width/height. Native drag-to-resize and OS IME/clipboard are not certified by these captures. The live dimensions are parent-controlled; this is not yet the auto-growing application composer.',
        notes,
        links: [
          {
            label: 'Light',
            href: `../../light-${browser}/report/${filename}`,
          },
          { label: 'Dark', href: `../../dark-${browser}/report/${filename}` },
          {
            label:
              browser === 'webkit' ? 'Chromium reference' : 'WebKit reference',
            href: `../../${theme}-${browser === 'webkit' ? 'chromium' : 'webkit'}/report/${filename}`,
          },
          { label: 'All multiline checkpoints', href: 'report.html' },
        ],
      });
      await writeFile(resolve(folder, `report/${filename}`), html);
      console.log(resolve(folder, `report/${filename}`));
    }
  }
}
if (import.meta.main) {
  const args = process.argv.slice(2);
  const feedback = args[0] === '--feedback';
  if (feedback) args.shift();
  if (args.length > 1 || args[0]?.startsWith('--'))
    throw new Error(
      'Usage: component-review.ts [--feedback] [output-directory]',
    );
  await reviewTextarea(args[0], feedback);
}

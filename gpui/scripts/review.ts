import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import type { Suite } from '../contract';
import type { Row } from './compare';
import { renderReport } from './report';
import { assertCapturePair } from './provenance';

export const feedbackNotes: Record<string, string> = {
  'button-disabled-hover/hover':
    'Disabled styling: one pale composited fill/border and pale label. This also fixes primary-disabled idle/click and disabled focus-blocked; those equivalent cases are not repeated here.',
  'button-cancelled-press/held-outside':
    'Focus is retained after pressing and moving outside, but activation is cancelled. The accent border indicates focus, not a held/activated state. Kept the native focus treatment you preferred; confirm it also makes sense here.',
  'button-cancelled-press/blurred-pointer':
    'Pointer is still over the button after blur. Both show hover here. Compare with the next two checkpoints to review the remaining hover policy choice.',
  'button-cancelled-press/blurred-key':
    'Intentional GPUI behavior: keyboard use suppresses hover styling even when the pointer stays over the button. Vue retains hover. No synthetic mouse move was added to hide this difference; choose whether to keep the native behavior.',
  'button-cancelled-press/hover-restored':
    'A real pointer move restores hover. Click count remains unchanged through the cancelled press and blurred key sequence.',
  'input-placeholder/idle':
    'Placeholder: removed native CoreGraphics stroke thickening, matching the browser antialiased policy without changing font weight or colour tokens.',
  'input-insert/inserted':
    'Representative normal input text after the same rasterization correction; replacement, deletion and other text cases use the same fix. Kept the thinner border without the extra outer ring you accepted.',
  'input-emoji/typed':
    'The native emoji matches the size of Vue in WebKit (the shipping macOS engine), shown alongside. Chromium uses a smaller glyph. Kept the macOS fallback size rather than guessing a scale factor; a small raster/baseline difference remains. The first Vue column is Chromium.',
  'input-caret-boundaries/latin':
    'Real carets are visible on both sides. Native caret is now 1 logical px wide (was 1.5), centered on the insertion boundary instead of extending into the next glyph.',
  'input-caret-boundaries/before-emoji':
    'Caret immediately before the emoji, at UTF-16 offset 2. This is a real editable Base/browser control, not a drawn overlay.',
  'input-caret-boundaries/after-emoji':
    'Caret immediately after the emoji, at UTF-16 offset 4. Review spacing against the next Latin glyph.',
  'input-caret-boundaries/end':
    'Caret at the end of the value, UTF-16 offset 6. Geometry changes do not change selection or editing offsets.',
  'input-placeholder-caret/visible':
    'Both real carets are visible at the start of an empty field. Also fixed a Base focus-notification bug that could start blinking twice and initially hide the caret.',
  'input-tab-focus/disabled-blur':
    'Selection remains visibly painted after disabling/blurring, without restoring focus or allowing typing. The skipped-Tab checkpoint has the same fix and is covered by the full regression suite.',
};

export function feedbackRows(rows: Row[]): Row[] {
  return Object.keys(feedbackNotes).map((id) => {
    const matches = rows.filter((row) => row.id === id);
    if (matches.length !== 1)
      throw new Error(`feedback report requires exactly one ${id}`);
    return matches[0]!;
  });
}

export async function review(
  base = resolve(import.meta.dir, '../results/follow-up'),
): Promise<void> {
  for (const theme of ['light', 'dark'] as const) {
    const folder = resolve(base, `${theme}-chromium`);
    const suite: Suite = JSON.parse(
      await readFile(resolve(folder, 'request.json'), 'utf8'),
    );
    const webkitFolder = resolve(base, `${theme}-webkit`);
    assertCapturePair(
      JSON.parse(await readFile(resolve(folder, 'capture.json'), 'utf8')),
      JSON.parse(await readFile(resolve(webkitFolder, 'capture.json'), 'utf8')),
    );
    const webkitSuite = JSON.parse(
      await readFile(resolve(webkitFolder, 'request.json'), 'utf8'),
    );
    if (JSON.stringify(suite) !== JSON.stringify(webkitSuite))
      throw new Error('follow-up requires matching Chromium/WebKit suites');
    await readFile(resolve(webkitFolder, 'report/vue--input-emoji--typed.png'));
    const report: { checkpoints: Row[] } = JSON.parse(
      await readFile(resolve(folder, 'report/report.json'), 'utf8'),
    );
    const html = renderReport(suite, feedbackRows(report.checkpoints), {
      title: `Review follow-up · ${theme}`,
      intro:
        'Only fixes and remaining choices from your review. Your preferred accent focus border, thinner native focus treatment and omitted input outer ring are retained, not repeatedly presented for approval. Pale heatmap pixels indicate tiny channel differences; exact diagnostics are still available.',
      notes: feedbackNotes,
      extraImages: {
        'input-emoji/typed': [
          {
            label: 'Vue (WebKit / macOS)',
            path: `../../${theme}-webkit/report/vue--input-emoji--typed.png`,
          },
        ],
      },
      links: [
        { label: 'Light', href: '../../light-chromium/report/feedback.html' },
        { label: 'Dark', href: '../../dark-chromium/report/feedback.html' },
        { label: 'Full regression report', href: 'report.html' },
      ],
    });
    await writeFile(resolve(folder, 'report/feedback.html'), html);
    console.log(resolve(folder, 'report/feedback.html'));
  }
}
if (import.meta.main) await review(process.argv[2]);

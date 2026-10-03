import type { Suite } from '../contract';
import type { Row } from './compare';

const escape = (value: string): string =>
  value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');

// Capture filenames are validated by the contract; encode each relative path segment for HTML URLs.
const link = (path: string): string =>
  escape(
    path
      .split('/')
      .map((part) => encodeURIComponent(part))
      .join('/'),
  );

export function renderReport(
  suite: Suite,
  rows: Row[],
  metadata: {
    title?: string;
    intro?: string;
    notes?: Record<string, string>;
    links?: { label: string; href: string }[];
    extraImages?: Record<string, { label: string; path: string }[]>;
  } = {},
): string {
  const failures = rows.filter((row) => row.state.length).length;
  const reviews = rows.filter((row) => row.review).length;
  const title = metadata.title ?? `Vue ↔ GPUI: ${suite.theme}`;
  return `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${escape(metadata.title ?? 'Vue ↔ GPUI comparison')}</title>
<style>
:root{font:14px/1.5 system-ui,sans-serif;color:#222;background:#f3f4f6}body{max-width:1100px;margin:0 auto;padding:24px}h1{margin:0}header p{color:#555} .controls{position:sticky;top:0;background:#f3f4f6;padding:12px 0;display:flex;gap:16px;align-items:center;flex-wrap:wrap;z-index:1}input[type=search]{font:inherit;padding:8px;width:min(340px,80vw)}button{font:inherit;cursor:pointer}article{background:white;border:1px solid #d9dce1;border-radius:8px;margin:16px 0;padding:20px}article h2{font-size:17px;margin:0 0 8px;overflow-wrap:anywhere}.fail{color:#a12525}.review{color:#825600}.pass{color:#276b44}pre{white-space:pre-wrap;overflow-wrap:anywhere;font:12px/1.5 ui-monospace,monospace;background:#f7f7f8;padding:10px;border-radius:4px}.images{display:flex;flex-wrap:wrap;gap:16px;align-items:flex-start}figure{margin:0}figcaption{font-weight:600;margin-bottom:6px}.images img{display:block;max-width:100%;border:1px solid #ccc;image-rendering:pixelated}a{color:#205b9b}.frames{margin-top:12px}body.zoom .images img{zoom:2}.metrics{color:#444;margin:8px 0}.note{color:#555}.images .exact{display:none}body.exact .images .exact{display:block}body.exact .images .heat{display:none} [hidden]{display:none!important}
</style>
<header><h1>${escape(title)}</h1><p>${rows.length} checkpoints · ${failures} state failures · ${reviews} visual review candidates. ${metadata.intro ? `${escape(metadata.intro)} ` : ''}All pixel counts are exact (including antialiasing); the heatmap only changes how differences look, not whether they count. No tolerance or alignment is applied. --strict-visual fails on any pixel or bounds difference.</p><p class="note">Images and crop origins are physical pixels at scale ${suite.scale} (${suite.scale} physical = 1 logical). Bounds and bounds deviation are logical pixels. ROI is the union of observed bounds + 4 logical px, then rounded out to physical pixels. Both crops use the same ROI; full frames remain linked.</p></header>
${metadata.links?.length ? `<nav>${metadata.links.map((item) => `<a href="${link(item.href)}">${escape(item.label)}</a>`).join(' · ')}</nav>` : ''}
<div class="controls"><input id="filter" type="search" placeholder="Filter scenario / checkpoint" aria-label="Filter scenario or checkpoint"><label><input id="failures" type="checkbox"> State failures only</label><label><input id="zoom" type="checkbox"> 2× review zoom</label><label><input id="exact" type="checkbox"> Exact mask</label><span id="count" aria-live="polite"></span></div>
<main>${rows
    .map(
      (r) =>
        `<article data-id="${escape(r.id.toLowerCase())}" data-failure="${r.state.length ? 'true' : 'false'}"><h2>${escape(r.id)} — <span class="${r.state.length ? 'fail' : 'pass'}">${r.state.length ? 'STATE FAIL' : 'STATE PASS'}</span> / <span class="${r.review ? 'review' : 'pass'}">${r.review ? 'VISUAL REVIEW REQUIRED' : 'pixel identical'}</span></h2>${metadata.notes?.[r.id] ? `<p><strong>Review:</strong> ${escape(metadata.notes[r.id]!)}</p>` : ''}<p class="note">Caret: ${r.caret === 'visible' ? 'actual visible caret on both sides' : 'hidden on both sides for deterministic comparison'}.</p>${r.state.length ? `<pre>${escape(r.state.join('\n'))}</pre>` : ''}<details><summary>Pixel diagnostics</summary><p class="metrics">ROI: ${r.visual.differingPixels.toLocaleString()} / ${r.visual.roiPixels.toLocaleString()} exact changed (${(r.visual.fraction * 100).toFixed(1)}%); mean RGB error ${r.visual.meanAbsoluteChannelError.toFixed(2)} / 255. Max-channel magnitude: ${r.visual.minorPixels.toLocaleString()} ≤2, ${r.visual.moderatePixels.toLocaleString()} 3–8, ${r.visual.majorPixels.toLocaleString()} &gt;8. Full frame: ${r.visual.fullFrameDifferingPixels.toLocaleString()} / ${r.visual.fullFramePixels.toLocaleString()} changed. Bounds max deviation: ${r.visual.boundsDifference.toFixed(3)} logical px.</p><p class="note">Crop origin (${r.cropX}, ${r.cropY}) physical px; size ${r.cropWidth} × ${r.cropHeight} physical px. ${r.visual.diagnosticTranslation.dx || r.visual.diagnosticTranslation.dy ? `Diagnostic GPUI sampling offset (${r.visual.diagnosticTranslation.dx}, ${r.visual.diagnosticTranslation.dy}) physical px relative to Vue reduces interior RGB error ${r.visual.diagnosticTranslation.errorReductionPercent.toFixed(1)}%. This is not used for comparison or approval.` : 'No improving translation within ±2 physical px on the fixed interior ROI.'}</p></details><div class="images">${[
          ['Vue', r.browserCrop, ''],
          ['GPUI', r.nativeCrop, ''],
          ...(metadata.extraImages?.[r.id]?.map((image) => [
            image.label,
            image.path,
            '',
          ]) ?? []),
          [
            'Magnitude heatmap (pale = small, vivid = large)',
            r.heatmap,
            'heat',
          ],
          ['Exact ROI mask (any changed pixel)', r.diff, 'exact'],
        ]
          .map(
            ([label, path, kind]) =>
              `<figure class="${kind}"><figcaption>${label}</figcaption><img src="${link(path!)}" width="${r.cropWidth / suite.scale}" height="${r.cropHeight / suite.scale}" alt="${label} crop for ${escape(r.id)}"></figure>`,
          )
          .join(
            '',
          )}</div><p class="frames">Full-frame originals: <a href="${link(r.browser)}">Vue</a> · <a href="${link(r.native)}">GPUI</a> · <a href="${link(`full--${r.diff}`)}">Full-frame diff</a></p></article>`,
    )
    .join('')}</main>
<script>
const search = document.querySelector('#filter'), failures = document.querySelector('#failures'), zoom = document.querySelector('#zoom'), exact = document.querySelector('#exact'), count = document.querySelector('#count');
const cards = [...document.querySelectorAll('article')];
function update() { const query = search.value.trim().toLowerCase(); let visible = 0; for (const card of cards) { card.hidden = !card.dataset.id.includes(query) || (failures.checked && card.dataset.failure !== 'true'); if (!card.hidden) visible++; } count.textContent = visible + ' / ' + cards.length + ' checkpoints'; document.body.classList.toggle('zoom', zoom.checked); document.body.classList.toggle('exact', exact.checked); }
for (const control of [search, failures, zoom, exact]) control.addEventListener('input', update);
update();
</script></html>`;
}

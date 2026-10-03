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

export function renderReport(suite: Suite, rows: Row[]): string {
  const failures = rows.filter((row) => row.state.length).length;
  const reviews = rows.filter((row) => row.review).length;
  return `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Vue ↔ GPUI comparison</title>
<style>
:root{font:14px/1.5 system-ui,sans-serif;color:#222;background:#f3f4f6}body{max-width:1100px;margin:0 auto;padding:24px}h1{margin:0}header p{color:#555} .controls{position:sticky;top:0;background:#f3f4f6;padding:12px 0;display:flex;gap:16px;align-items:center;flex-wrap:wrap;z-index:1}input[type=search]{font:inherit;padding:8px;width:min(340px,80vw)}button{font:inherit;cursor:pointer}article{background:white;border:1px solid #d9dce1;border-radius:8px;margin:16px 0;padding:20px}article h2{font-size:17px;margin:0 0 8px;overflow-wrap:anywhere}.fail{color:#a12525}.review{color:#825600}.pass{color:#276b44}pre{white-space:pre-wrap;overflow-wrap:anywhere;font:12px/1.5 ui-monospace,monospace;background:#f7f7f8;padding:10px;border-radius:4px}.images{display:flex;flex-wrap:wrap;gap:16px;align-items:flex-start}figure{margin:0}figcaption{font-weight:600;margin-bottom:6px}.images img{display:block;max-width:100%;border:1px solid #ccc;image-rendering:pixelated}a{color:#205b9b}.frames{margin-top:12px}body.zoom .images img{zoom:2} [hidden]{display:none!important}
</style>
<header><h1>Vue ↔ GPUI: ${escape(suite.theme)}</h1><p>${rows.length} checkpoints · ${failures} state failures · ${reviews} visual review candidates. ROI uses the union of control bounds + 4px. Full-frame metrics catch differences outside the ROI. 0-threshold including antialiasing; no visual tolerance is approved. Differences require human review. Bounds deviation is reported. Use --strict-visual to fail on every visual difference.</p></header>
<div class="controls"><input id="filter" type="search" placeholder="Filter scenario / checkpoint" aria-label="Filter scenario or checkpoint"><label><input id="failures" type="checkbox"> State failures only</label><label><input id="zoom" type="checkbox"> 2× review zoom</label><span id="count" aria-live="polite"></span></div>
<main>${rows
    .map(
      (r) =>
        `<article data-id="${escape(r.id.toLowerCase())}" data-failure="${r.state.length ? 'true' : 'false'}"><h2>${escape(r.id)} — <span class="${r.state.length ? 'fail' : 'pass'}">${r.state.length ? 'STATE FAIL' : 'STATE PASS'}</span> / <span class="${r.review ? 'review' : 'pass'}">${r.review ? 'VISUAL REVIEW REQUIRED' : 'pixel identical'}</span></h2><pre>${escape(r.state.join('\n') || 'No state differences')}\n${escape(JSON.stringify(r.visual))}</pre><div class="images">${[
          ['Vue', r.browserCrop],
          ['GPUI', r.nativeCrop],
          ['ROI diff', r.diff],
        ]
          .map(
            ([label, path]) =>
              `<figure><figcaption>${label}</figcaption><img src="${link(path!)}" width="${r.cropWidth / suite.scale}" height="${r.cropHeight / suite.scale}" alt="${label} crop for ${escape(r.id)}"></figure>`,
          )
          .join(
            '',
          )}</div><p class="frames">Full-frame originals: <a href="${link(r.browser)}">Vue</a> · <a href="${link(r.native)}">GPUI</a> · <a href="${link(`full--${r.diff}`)}">Full-frame diff</a></p></article>`,
    )
    .join('')}</main>
<script>
const search = document.querySelector('#filter'), failures = document.querySelector('#failures'), zoom = document.querySelector('#zoom'), count = document.querySelector('#count');
const cards = [...document.querySelectorAll('article')];
function update() { const query = search.value.trim().toLowerCase(); let visible = 0; for (const card of cards) { card.hidden = !card.dataset.id.includes(query) || (failures.checked && card.dataset.failure !== 'true'); if (!card.hidden) visible++; } count.textContent = visible + ' / ' + cards.length + ' checkpoints'; document.body.classList.toggle('zoom', zoom.checked); }
for (const control of [search, failures, zoom]) control.addEventListener('input', update);
update();
</script></html>`;
}

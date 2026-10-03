import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import { PNG } from 'pngjs';
import pixelmatch from 'pixelmatch';
import { renderReport } from './report';
import {
  assertCaptures,
  stateDifferences,
  type Bounds,
  type Capture,
  type Suite,
} from '../contract';

export type Row = {
  id: string;
  caret?: 'hidden' | 'visible';
  state: string[];
  visual: {
    differingPixels: number;
    roiPixels: number;
    fraction: number;
    meanAbsoluteChannelError: number;
    minorPixels: number;
    moderatePixels: number;
    majorPixels: number;
    diagnosticTranslation: {
      dx: number;
      dy: number;
      errorReductionPercent: number;
    };
    boundsDifference: number;
    fullFrameDifferingPixels: number;
    fullFramePixels: number;
    fullFrameFraction: number;
    fullFrameMeanAbsoluteChannelError: number;
  };
  browser: string;
  native: string;
  diff: string;
  heatmap: string;
  cropX: number;
  cropY: number;
  browserCrop: string;
  nativeCrop: string;
  cropWidth: number;
  cropHeight: number;
  review: boolean;
};
export function roi(
  a: Bounds,
  b: Bounds,
  width: number,
  height: number,
  scale: number,
): Bounds {
  const x = Math.max(0, Math.floor((Math.min(a.x, b.x) - 4) * scale));
  const y = Math.max(0, Math.floor((Math.min(a.y, b.y) - 4) * scale));
  return {
    x,
    y,
    width:
      Math.min(
        width,
        Math.ceil((Math.max(a.x + a.width, b.x + b.width) + 4) * scale),
      ) - x,
    height:
      Math.min(
        height,
        Math.ceil((Math.max(a.y + a.height, b.y + b.height) + 4) * scale),
      ) - y,
  };
}
export function pixelMetrics(
  a: PNG,
  b: PNG,
  region: Bounds,
): {
  differingPixels: number;
  roiPixels: number;
  fraction: number;
  meanAbsoluteChannelError: number;
  minorPixels: number;
  moderatePixels: number;
  majorPixels: number;
  diff: PNG;
  heatmap: PNG;
} {
  if (a.width !== b.width || a.height !== b.height)
    throw new Error(
      `render size mismatch: ${a.width}x${a.height} vs ${b.width}x${b.height}`,
    );
  const w = region.width,
    h = region.height;
  if (w <= 0 || h <= 0) throw new Error('empty comparison ROI');
  const left = new PNG({ width: w, height: h });
  const right = new PNG({ width: w, height: h });
  const diff = new PNG({ width: w, height: h });
  const heatmap = new PNG({ width: w, height: h });
  let error = 0,
    minorPixels = 0,
    moderatePixels = 0,
    majorPixels = 0,
    exactPixels = 0;
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      const src = ((y + region.y) * a.width + x + region.x) * 4,
        dst = (y * w + x) * 4;
      let magnitude = 0;
      for (let c = 0; c < 4; c++) {
        left.data[dst + c] = a.data[src + c]!;
        right.data[dst + c] = b.data[src + c]!;
        const delta = Math.abs(a.data[src + c]! - b.data[src + c]!);
        if (c < 3) error += delta;
        magnitude = Math.max(magnitude, delta);
      }
      if (magnitude > 0) exactPixels++;
      if (magnitude <= 2 && magnitude > 0) minorPixels++;
      else if (magnitude <= 8 && magnitude > 0) moderatePixels++;
      else if (magnitude > 8) majorPixels++;
      // White means identical; small channel rounding stays pale, large changes are vivid.
      const intensity = Math.sqrt(magnitude / 255);
      heatmap.data[dst] = Math.round(255 - 75 * intensity);
      heatmap.data[dst + 1] = Math.round(255 - 245 * intensity);
      heatmap.data[dst + 2] = Math.round(255 - 220 * intensity);
      heatmap.data[dst + 3] = 255;
    }
  // Keep the familiar mask backdrop, but raw RGBA differences define exactness.
  pixelmatch(left.data, right.data, diff.data, w, h, {
    threshold: 0,
    includeAA: true,
  });
  for (let i = 0; i < left.data.length; i += 4) {
    if (
      left.data[i] !== right.data[i] ||
      left.data[i + 1] !== right.data[i + 1] ||
      left.data[i + 2] !== right.data[i + 2] ||
      left.data[i + 3] !== right.data[i + 3]
    ) {
      diff.data[i] = 255;
      diff.data[i + 1] = 0;
      diff.data[i + 2] = 0;
      diff.data[i + 3] = 255;
    }
  }
  return {
    differingPixels: exactPixels,
    roiPixels: w * h,
    fraction: exactPixels / (w * h),
    meanAbsoluteChannelError: error / (w * h * 3),
    minorPixels,
    moderatePixels,
    majorPixels,
    diff,
    heatmap,
  };
}
// Diagnostic only: fixed interior coordinates for every candidate; no shifted image is saved or used to pass.
export function diagnosticTranslation(
  a: PNG,
  b: PNG,
  region: Bounds,
): {
  dx: number;
  dy: number;
  errorReductionPercent: number;
} {
  const margin = 2;
  if (region.width <= margin * 2 || region.height <= margin * 2)
    return { dx: 0, dy: 0, errorReductionPercent: 0 };
  let baseline = 0,
    best = Infinity,
    dx = 0,
    dy = 0;
  for (let shiftY = -margin; shiftY <= margin; shiftY++)
    for (let shiftX = -margin; shiftX <= margin; shiftX++) {
      let error = 0;
      for (
        let y = region.y + margin;
        y < region.y + region.height - margin;
        y++
      )
        for (
          let x = region.x + margin;
          x < region.x + region.width - margin;
          x++
        ) {
          const left = (y * a.width + x) * 4;
          const right = ((y + shiftY) * b.width + x + shiftX) * 4;
          for (let c = 0; c < 3; c++)
            error += Math.abs(a.data[left + c]! - b.data[right + c]!);
        }
      if (shiftX === 0 && shiftY === 0) baseline = error;
      if (error < best) {
        best = error;
        dx = shiftX;
        dy = shiftY;
      }
    }
  if (best >= baseline) return { dx: 0, dy: 0, errorReductionPercent: 0 };
  return {
    dx,
    dy,
    errorReductionPercent: (100 * (baseline - best)) / baseline,
  };
}
function crop(source: PNG, region: Bounds): PNG {
  const result = new PNG({ width: region.width, height: region.height });
  for (let y = 0; y < region.height; y++) {
    const start = ((region.y + y) * source.width + region.x) * 4;
    source.data.copy(
      result.data,
      y * region.width * 4,
      start,
      start + region.width * 4,
    );
  }
  return result;
}
async function image(
  path: string,
  width: number,
  height: number,
): Promise<PNG> {
  const png = PNG.sync.read(await readFile(path));
  if (png.width !== width || png.height !== height)
    throw new Error(
      `${path}: render size mismatch: expected ${width}x${height}, got ${png.width}x${png.height}`,
    );
  return png;
}
export async function compare(
  suite: Suite,
  reference: Capture[],
  native: Capture[],
  browserDir: string,
  nativeDir: string,
  out: string,
  strictVisual = false,
): Promise<Row[]> {
  const browser = assertCaptures(suite, reference, 'browser'),
    gpui = assertCaptures(suite, native, 'native');
  await mkdir(out, { recursive: true });
  const rows: Row[] = [];
  for (const scenario of suite.scenarios)
    for (const step of scenario.steps) {
      if (step.action !== 'checkpoint') continue;
      const id = `${scenario.id}/${step.name}`,
        a = browser.get(id)!,
        b = gpui.get(id)!;
      const state = [
        ...stateDifferences(step.expected, a.observation).map(
          (s) => `Vue vs expected: ${s}`,
        ),
        ...stateDifferences(step.expected, b.observation).map(
          (s) => `GPUI vs expected: ${s}`,
        ),
        ...stateDifferences(a.observation, b.observation).map(
          (s) => `Vue vs GPUI: ${s}`,
        ),
      ];
      const srcA = await image(
        resolve(browserDir, a.screenshot),
        suite.viewport.width * suite.scale,
        suite.viewport.height * suite.scale,
      );
      const srcB = await image(
        resolve(nativeDir, b.screenshot),
        suite.viewport.width * suite.scale,
        suite.viewport.height * suite.scale,
      );
      const region = roi(
        a.bounds,
        b.bounds,
        srcA.width,
        srcA.height,
        suite.scale,
      );
      const { diff, heatmap, ...metrics } = pixelMetrics(srcA, srcB, region);
      const fullMetrics = pixelMetrics(srcA, srcB, {
        x: 0,
        y: 0,
        width: srcA.width,
        height: srcA.height,
      });
      const diffFile = `${a.screenshot}`;
      await writeFile(
        resolve(out, `full--${diffFile}`),
        PNG.sync.write(fullMetrics.diff),
      );
      await writeFile(resolve(out, diffFile), PNG.sync.write(diff));
      await writeFile(
        resolve(out, `heat--${diffFile}`),
        PNG.sync.write(heatmap),
      );
      await writeFile(
        resolve(out, `vue--${diffFile}`),
        PNG.sync.write(crop(srcA, region)),
      );
      await writeFile(
        resolve(out, `gpui--${diffFile}`),
        PNG.sync.write(crop(srcB, region)),
      );
      const boundsDifference = Math.max(
        ...(['x', 'y', 'width', 'height'] as const).map((k) =>
          Math.abs(a.bounds[k] - b.bounds[k]),
        ),
      );
      rows.push({
        id,
        caret: step.caret ?? 'hidden',
        state,
        visual: {
          ...metrics,
          diagnosticTranslation: diagnosticTranslation(srcA, srcB, region),
          boundsDifference,
          fullFrameDifferingPixels: fullMetrics.differingPixels,
          fullFramePixels: fullMetrics.roiPixels,
          fullFrameFraction: fullMetrics.fraction,
          fullFrameMeanAbsoluteChannelError:
            fullMetrics.meanAbsoluteChannelError,
        },
        browser: `../${basename(browserDir)}/${a.screenshot}`,
        native: `../${basename(nativeDir)}/${b.screenshot}`,
        diff: diffFile,
        heatmap: `heat--${diffFile}`,
        cropX: region.x,
        cropY: region.y,
        browserCrop: `vue--${diffFile}`,
        nativeCrop: `gpui--${diffFile}`,
        cropWidth: region.width,
        cropHeight: region.height,
        review: fullMetrics.differingPixels > 0 || boundsDifference > 0,
      });
    }
  const stateFailures = rows.filter((r) => r.state.length).length,
    visualReviews = rows.filter((r) => r.review).length;
  await writeFile(resolve(out, 'report.html'), renderReport(suite, rows));
  await writeFile(
    resolve(out, 'report.json'),
    JSON.stringify(
      {
        stateFailures,
        visualReviews,
        status: stateFailures ? 'STATE FAIL' : 'STATE PASS',
        visualStatus: visualReviews
          ? 'VISUAL REVIEW REQUIRED'
          : 'PIXEL IDENTICAL',
        checkpoints: rows,
      },
      null,
      2,
    ),
  );
  console.log(
    `STATE ${stateFailures ? 'FAIL' : 'PASS'} (${stateFailures} failures / ${rows.length} checkpoints); ${visualReviews ? 'VISUAL REVIEW REQUIRED' : 'PIXEL IDENTICAL'} (${visualReviews}/${rows.length}) — ${resolve(out, 'report.html')}`,
  );
  if (stateFailures || (strictVisual && visualReviews))
    throw new Error(
      `${stateFailures} state failures, ${visualReviews} visual review candidates (report: ${resolve(out, 'report.html')})`,
    );
  return rows;
}

import { describe, expect, it } from 'vitest';

import {
  MAX_SCALE,
  cameraViewBox,
  fitDiagram,
  panDiagram,
  resizeDiagram,
  zoomDiagram,
} from './diagram-viewport';

const bounds = { x: 0, y: 0, width: 1_200, height: 600 };
const viewport = { width: 1_000, height: 700 };

describe('diagram viewport', () => {
  it('fits at the drawing natural scale or smaller and centres it', () => {
    const camera = fitDiagram(bounds, viewport);
    const box = cameraViewBox(camera, viewport);

    expect(camera.scale).toBeCloseTo(904 / 1_200);
    expect(-box.x).toBeCloseTo(box.x + box.width - bounds.width);
    expect(-box.y).toBeCloseTo(box.y + box.height - bounds.height);
  });

  it('centres bounds whose coordinate origin is not zero', () => {
    const offsetBounds = { x: -400, y: 250, width: 1_200, height: 600 };
    const box = cameraViewBox(fitDiagram(offsetBounds, viewport), viewport);

    expect(offsetBounds.x - box.x).toBeCloseTo(
      box.x + box.width - offsetBounds.x - offsetBounds.width,
    );
    expect(offsetBounds.y - box.y).toBeCloseTo(
      box.y + box.height - offsetBounds.y - offsetBounds.height,
    );
  });

  it('keeps the diagram coordinate beneath the pointer fixed while zooming', () => {
    const camera = fitDiagram(bounds, viewport);
    const point = { x: 320, y: 240 };
    const before = {
      x: (point.x - camera.tx) / camera.scale,
      y: (point.y - camera.ty) / camera.scale,
    };
    const zoomed = zoomDiagram(camera, point, camera.scale * 2);
    const after = {
      x: (point.x - zoomed.tx) / zoomed.scale,
      y: (point.y - zoomed.ty) / zoomed.scale,
    };

    expect(after).toEqual(before);
  });

  it('bounds zoom while still fitting very large and small drawings', () => {
    const largeFit = fitDiagram(
      { x: 0, y: 0, width: 20_000, height: 10_000 },
      viewport,
    );
    const smallFit = fitDiagram(
      { x: 0, y: 0, width: 0.01, height: 0.005 },
      viewport,
    );
    const minimumScale = Math.min(0.1, largeFit.scale);

    expect(smallFit.scale).toBe(1.5);
    expect(zoomDiagram(largeFit, { x: 0, y: 0 }, 100).scale).toBe(MAX_SCALE);
    expect(zoomDiagram(largeFit, { x: 0, y: 0 }, 0, minimumScale).scale).toBe(
      largeFit.scale,
    );
  });

  it('keeps the world point at the viewport centre fixed while resizing', () => {
    const camera = fitDiagram(bounds, viewport);
    const next = { width: 1_300, height: 500 };
    const resized = resizeDiagram(camera, viewport, next);
    const before = cameraViewBox(camera, viewport);
    const after = cameraViewBox(resized, next);

    expect(after.x + after.width / 2).toBeCloseTo(before.x + before.width / 2);
    expect(after.y + after.height / 2).toBeCloseTo(
      before.y + before.height / 2,
    );
    expect(resized.scale).toBe(camera.scale);
  });

  it('pans by moving the SVG viewBox in the opposite direction', () => {
    const camera = fitDiagram(bounds, viewport);
    const before = cameraViewBox(camera, viewport);
    const after = cameraViewBox(panDiagram(camera, -40, -60), viewport);

    expect(after.x).toBeGreaterThan(before.x);
    expect(after.y).toBeGreaterThan(before.y);
    expect(after.width).toBe(before.width);
    expect(after.height).toBe(before.height);
  });
});

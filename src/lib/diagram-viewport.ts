interface Point {
  x: number;
  y: number;
}

interface Size {
  width: number;
  height: number;
}

interface DiagramBounds extends Size {
  x: number;
  y: number;
}

/** Screen-space placement of the diagram's natural coordinate system. */
interface DiagramCamera {
  scale: number;
  tx: number;
  ty: number;
}

type ViewBox = DiagramBounds;

const MIN_SCALE = 0.1;
const MAX_SCALE = 8;
const MAX_FIT_SCALE = 1.5;
const FIT_MARGIN = 96;

/** Places the whole drawing in view without enlarging small drawings excessively. */
function fitDiagram(bounds: DiagramBounds, viewport: Size): DiagramCamera {
  const availableWidth = Math.max(1, viewport.width - FIT_MARGIN);
  const availableHeight = Math.max(1, viewport.height - FIT_MARGIN);
  const scale = Math.min(
    availableWidth / bounds.width,
    availableHeight / bounds.height,
    MAX_FIT_SCALE,
  );
  return {
    scale,
    tx: (viewport.width - bounds.width * scale) / 2 - bounds.x * scale,
    ty: (viewport.height - bounds.height * scale) / 2 - bounds.y * scale,
  };
}

/** Rescales around a fixed screen point, keeping the world point beneath it fixed. */
function zoomDiagram(
  camera: DiagramCamera,
  point: Point,
  nextScale: number,
  minimumScale = MIN_SCALE,
): DiagramCamera {
  const scale = Math.min(MAX_SCALE, Math.max(minimumScale, nextScale));
  const ratio = scale / camera.scale;
  return {
    scale,
    tx: point.x - (point.x - camera.tx) * ratio,
    ty: point.y - (point.y - camera.ty) * ratio,
  };
}

function panDiagram(
  camera: DiagramCamera,
  deltaX: number,
  deltaY: number,
): DiagramCamera {
  return {
    ...camera,
    tx: camera.tx + deltaX,
    ty: camera.ty + deltaY,
  };
}

/** Keeps the world point at the viewport centre fixed while it changes size. */
function resizeDiagram(
  camera: DiagramCamera,
  previous: Size,
  next: Size,
): DiagramCamera {
  return panDiagram(
    camera,
    (next.width - previous.width) / 2,
    (next.height - previous.height) / 2,
  );
}

/**
 * Expresses the camera as an SVG viewport. Changing viewBox makes the browser
 * lay out and paint vectors at the requested resolution; unlike a CSS transform,
 * it does not ask the compositor to enlarge a cached layer.
 */
function cameraViewBox(camera: DiagramCamera, viewport: Size): ViewBox {
  return {
    x: -camera.tx / camera.scale,
    y: -camera.ty / camera.scale,
    width: viewport.width / camera.scale,
    height: viewport.height / camera.scale,
  };
}

export type { DiagramBounds, DiagramCamera, Point, Size, ViewBox };
export {
  MAX_SCALE,
  MIN_SCALE,
  cameraViewBox,
  fitDiagram,
  panDiagram,
  resizeDiagram,
  zoomDiagram,
};

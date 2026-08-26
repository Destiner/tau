<template>
  <DialogRoot
    open
    @update:open="handleOpenChange"
  >
    <DialogPortal>
      <DialogContent
        class="diagram-viewer"
        @close-auto-focus="handleCloseAutoFocus"
      >
        <VisuallyHidden>
          <DialogTitle>Diagram</DialogTitle>
        </VisuallyHidden>
        <div
          ref="canvas"
          class="viewer-canvas"
          :class="{ panning }"
          :style="{ backgroundPosition }"
          @wheel.prevent="handleWheel"
          @pointerdown="handlePointerDown"
          @pointermove="handlePointerMove"
          @pointerup="handlePointerUp"
          @pointercancel="handlePointerCancel"
        >
          <!-- eslint-disable vue/no-v-html -- the svg is renderDiagram's own sanitized output -->
          <div
            ref="drawing"
            class="viewer-drawing"
            v-html="svg"
          ></div>
          <!-- eslint-enable vue/no-v-html -->
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<script setup lang="ts">
import {
  DialogContent,
  DialogPortal,
  DialogRoot,
  DialogTitle,
  VisuallyHidden,
} from 'reka-ui';
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue';

import {
  MIN_SCALE,
  cameraViewBox,
  fitDiagram,
  panDiagram,
  resizeDiagram,
  zoomDiagram,
  type DiagramBounds,
  type DiagramCamera,
  type Point,
  type Size,
} from '../../lib/diagram-viewport';

const props = defineProps<{
  /** The drawn diagram, at the natural size its width and height name. */
  svg: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** Persistent control that receives focus after the viewer closes. */
  returnFocus?: () => HTMLElement | undefined;
}>();

const emit = defineEmits<{ close: [] }>();

const canvas = ref<HTMLElement | null>(null);
const drawing = ref<HTMLElement | null>(null);
const panning = ref(false);
const backgroundPosition = ref('0px 0px');

/** Under this much travel a press is a click, which is how the viewer closes. */
const CLICK_SLOP = 5;

let camera: DiagramCamera = { scale: 1, tx: 0, ty: 0 };
let viewport: Size = { width: 1, height: 1 };
let minimumScale = MIN_SCALE;
let vector: SVGSVGElement | null = null;
let renderFrame = 0;
let resizeObserver: ResizeObserver | undefined;
let mounted = false;

function bounds(): DiagramBounds {
  return {
    x: props.x,
    y: props.y,
    width: props.width,
    height: props.height,
  };
}

function handleOpenChange(open: boolean): void {
  if (!open) emit('close');
}

/**
 * Reka observes Escape on window, after document's bubble phase. The app also
 * owns document keyboard handling, so close here in document's capture phase:
 * this viewer owns Escape before it can reach any whole-app handler.
 */
function handleKeydownCapture(event: KeyboardEvent): void {
  if (event.key !== 'Escape') return;
  event.preventDefault();
  event.stopImmediatePropagation();
  emit('close');
}

function handleCloseAutoFocus(event: Event): void {
  const target = props.returnFocus?.();
  if (!target) return;
  event.preventDefault();
  target.focus({ preventScroll: true });
}

function canvasSize(): Size {
  return {
    width: Math.max(1, canvas.value?.clientWidth ?? window.innerWidth),
    height: Math.max(1, canvas.value?.clientHeight ?? window.innerHeight),
  };
}

/** Writes at most once per frame even when a trackpad sends events faster. */
function renderCamera(): void {
  renderFrame = 0;
  if (!vector) return;
  const box = cameraViewBox(camera, viewport);
  vector.setAttribute(
    'viewBox',
    `${box.x} ${box.y} ${box.width} ${box.height}`,
  );
  backgroundPosition.value = `${camera.tx}px ${camera.ty}px`;
}

function scheduleRender(): void {
  if (!renderFrame) renderFrame = requestAnimationFrame(renderCamera);
}

/** Rescales around a fixed point, which is what keeps it under the pointer. */
function zoomTo(point: Point, next: number): void {
  camera = zoomDiagram(camera, point, next, minimumScale);
  scheduleRender();
}

function localPoint(clientX: number, clientY: number): Point {
  const box = canvas.value?.getBoundingClientRect();
  return {
    x: clientX - (box?.left ?? 0),
    y: clientY - (box?.top ?? 0),
  };
}

/**
 * Trackpads speak two dialects. Chromium reports a pinch as a wheel event
 * with ctrlKey set, and a two-finger drag as plain wheel deltas: the pinch
 * zooms around the pointer, the drag pans.
 */
function handleWheel(event: WheelEvent): void {
  syncViewport();
  if (event.ctrlKey || event.metaKey) {
    zoomTo(
      localPoint(event.clientX, event.clientY),
      camera.scale * Math.exp(-event.deltaY * 0.01),
    );
  } else {
    camera = panDiagram(camera, -event.deltaX, -event.deltaY);
    scheduleRender();
  }
}

/* WebKit — the webview the app ships in — reports a pinch as gesture events. */
type WebKitGestureEvent = Event & {
  scale: number;
  clientX: number;
  clientY: number;
};

let gestureBase = 1;

function handleGestureStart(event: Event): void {
  event.preventDefault();
  syncViewport();
  gestureBase = camera.scale;
}

function handleGestureChange(event: Event): void {
  event.preventDefault();
  const gesture = event as WebKitGestureEvent;
  zoomTo(
    localPoint(gesture.clientX, gesture.clientY),
    gestureBase * gesture.scale,
  );
}

let lastX = 0;
let lastY = 0;
let downX = 0;
let downY = 0;

function handlePointerDown(event: PointerEvent): void {
  if (event.button !== 0) return;
  syncViewport();
  panning.value = true;
  lastX = event.clientX;
  lastY = event.clientY;
  downX = event.clientX;
  downY = event.clientY;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}

function handlePointerMove(event: PointerEvent): void {
  if (!panning.value) return;
  camera = panDiagram(camera, event.clientX - lastX, event.clientY - lastY);
  lastX = event.clientX;
  lastY = event.clientY;
  scheduleRender();
}

function handlePointerUp(event: PointerEvent): void {
  if (!panning.value) return;
  panning.value = false;
  // The viewer wears no chrome, so a press that never became a pan is the
  // exit — the same gesture that leaves any fullscreen preview.
  if (Math.hypot(event.clientX - downX, event.clientY - downY) < CLICK_SLOP) {
    emit('close');
  }
}

function handlePointerCancel(): void {
  panning.value = false;
}

/** Keeps the same diagram point centred if the fullscreen viewport changes. */
function syncViewport(): void {
  const next = canvasSize();
  if (next.width === viewport.width && next.height === viewport.height) return;
  camera = resizeDiagram(camera, viewport, next);
  viewport = next;
  scheduleRender();
}

onMounted(async () => {
  mounted = true;
  document.addEventListener('keydown', handleKeydownCapture, true);
  await nextTick();
  if (!mounted) return;
  vector = drawing.value?.querySelector('svg') ?? null;
  if (!vector) return;

  // The outer SVG is the viewport. viewBox changes make both engines lay out
  // and paint its vectors afresh instead of scaling a composited bitmap layer.
  vector.setAttribute('width', '100%');
  vector.setAttribute('height', '100%');
  vector.setAttribute('preserveAspectRatio', 'none');

  viewport = canvasSize();
  camera = fitDiagram(bounds(), viewport);
  minimumScale = Math.min(MIN_SCALE, camera.scale);
  renderCamera();

  canvas.value?.addEventListener('gesturestart', handleGestureStart);
  canvas.value?.addEventListener('gesturechange', handleGestureChange);
  resizeObserver = new ResizeObserver(syncViewport);
  if (canvas.value) resizeObserver.observe(canvas.value);
});

onBeforeUnmount(() => {
  mounted = false;
  document.removeEventListener('keydown', handleKeydownCapture, true);
  canvas.value?.removeEventListener('gesturestart', handleGestureStart);
  canvas.value?.removeEventListener('gesturechange', handleGestureChange);
  resizeObserver?.disconnect();
  cancelAnimationFrame(renderFrame);
});
</script>

<style scoped>
/* Over every panel and dialog: a diagram inside a dialog still expands on top. */
:global(.diagram-viewer) {
  position: fixed;
  z-index: 30;
  inset: 0;
}

/*
 * The page the figure floats on. The dot grid rides the pan, which is what
 * tells the eye the surface moved rather than the figure resizing.
 */
:global(.diagram-viewer .viewer-canvas) {
  position: absolute;
  overflow: hidden;
  background-color: var(--canvas);
  background-image: radial-gradient(var(--border) 1px, transparent 1px);
  background-size: 24px 24px;
  inset: 0;
  touch-action: none;
}

:global(.diagram-viewer .viewer-canvas.panning) {
  cursor: grabbing;
}

:global(.diagram-viewer .viewer-drawing),
:global(.diagram-viewer .viewer-drawing > svg) {
  display: block;
  width: 100%;
  height: 100%;
}
</style>

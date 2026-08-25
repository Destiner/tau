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
          :style="{ backgroundPosition: `${tx}px ${ty}px` }"
          @wheel.prevent="handleWheel"
          @pointerdown="handlePointerDown"
          @pointermove="handlePointerMove"
          @pointerup="handlePointerUp"
          @pointercancel="handlePointerCancel"
        >
          <!-- eslint-disable vue/no-v-html -- the svg is renderDiagram's own sanitized output -->
          <div
            class="viewer-plane"
            :style="planeStyle"
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
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';

const props = defineProps<{
  /** The drawn diagram, at the natural size its width and height name. */
  svg: string;
  width: number;
  height: number;
  /** Persistent control that receives focus after the viewer closes. */
  returnFocus?: () => HTMLElement | undefined;
}>();

const emit = defineEmits<{ close: [] }>();

const canvas = ref<HTMLElement | null>(null);

const scale = ref(1);
const tx = ref(0);
const ty = ref(0);
const panning = ref(false);

const MIN_SCALE = 0.1;
const MAX_SCALE = 8;
/** Fitting may enlarge a small diagram — it is vector — but not comically. */
const MAX_FIT_SCALE = 1.5;
/** Breathing room between the figure and the screen edge when fitting. */
const FIT_MARGIN = 96;
/** Under this much travel a press is a click, which is how the viewer closes. */
const CLICK_SLOP = 5;

const planeStyle = computed(() => ({
  width: `${props.width}px`,
  height: `${props.height}px`,
  transform: `translate(${tx.value}px, ${ty.value}px) scale(${scale.value})`,
}));

function handleOpenChange(open: boolean): void {
  if (!open) emit('close');
}

function handleCloseAutoFocus(event: Event): void {
  const target = props.returnFocus?.();
  if (!target) return;
  event.preventDefault();
  target.focus({ preventScroll: true });
}

function fitScale(): number {
  const availableWidth = window.innerWidth - FIT_MARGIN;
  const availableHeight = window.innerHeight - FIT_MARGIN;
  return Math.min(
    availableWidth / props.width,
    availableHeight / props.height,
    MAX_FIT_SCALE,
  );
}

/** Opens on the whole figure, centered: the overview the transcript could not give. */
function fit(): void {
  const at = fitScale();
  scale.value = at;
  tx.value = (window.innerWidth - props.width * at) / 2;
  ty.value = (window.innerHeight - props.height * at) / 2;
}

/** Rescales around a fixed point, which is what keeps it under the pointer. */
function zoomTo(pointX: number, pointY: number, next: number): void {
  const clamped = Math.min(MAX_SCALE, Math.max(MIN_SCALE, next));
  const ratio = clamped / scale.value;
  tx.value = pointX - (pointX - tx.value) * ratio;
  ty.value = pointY - (pointY - ty.value) * ratio;
  scale.value = clamped;
}

/**
 * Trackpads speak two dialects. Chromium reports a pinch as a wheel event
 * with ctrlKey set, and a two-finger drag as plain wheel deltas: the pinch
 * zooms around the pointer, the drag pans.
 */
function handleWheel(event: WheelEvent): void {
  if (event.ctrlKey || event.metaKey) {
    zoomTo(
      event.clientX,
      event.clientY,
      scale.value * Math.exp(-event.deltaY * 0.01),
    );
  } else {
    tx.value -= event.deltaX;
    ty.value -= event.deltaY;
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
  gestureBase = scale.value;
}

function handleGestureChange(event: Event): void {
  event.preventDefault();
  const gesture = event as WebKitGestureEvent;
  zoomTo(gesture.clientX, gesture.clientY, gestureBase * gesture.scale);
}

let lastX = 0;
let lastY = 0;
let downX = 0;
let downY = 0;

function handlePointerDown(event: PointerEvent): void {
  if (event.button !== 0) return;
  panning.value = true;
  lastX = event.clientX;
  lastY = event.clientY;
  downX = event.clientX;
  downY = event.clientY;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}

function handlePointerMove(event: PointerEvent): void {
  if (!panning.value) return;
  tx.value += event.clientX - lastX;
  ty.value += event.clientY - lastY;
  lastX = event.clientX;
  lastY = event.clientY;
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

onMounted(() => {
  fit();
  canvas.value?.addEventListener('gesturestart', handleGestureStart);
  canvas.value?.addEventListener('gesturechange', handleGestureChange);
});

onBeforeUnmount(() => {
  canvas.value?.removeEventListener('gesturestart', handleGestureStart);
  canvas.value?.removeEventListener('gesturechange', handleGestureChange);
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

:global(.diagram-viewer .viewer-plane) {
  position: absolute;
  top: 0;
  left: 0;
  transform-origin: 0 0;
  will-change: transform;
}

:global(.diagram-viewer .viewer-plane svg) {
  display: block;
  width: 100%;
  height: 100%;
}
</style>

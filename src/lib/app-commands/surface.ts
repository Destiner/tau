import { ref } from 'vue';

// The palette is the top interactive layer even when its focus moves outside
// a Reka popover's subtree. Keep the underlying form mounted until it closes.
const paletteLayerOpen = ref(false);

function preserveSurfaceUnderPalette(event: Event): void {
  if (paletteLayerOpen.value) event.preventDefault();
}

export { paletteLayerOpen, preserveSurfaceUnderPalette };

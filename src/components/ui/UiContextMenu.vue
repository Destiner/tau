<template>
  <!--
    The surface and the item are styled in ui/surface.css: reka portals this
    content to the body, where a scoped attribute never reaches it, and the two
    menus used to answer that by carrying a copy of the stylesheet each.
  -->
  <ContextMenuRoot>
    <ContextMenuTrigger
      as-child
      @contextmenu="rememberTrigger"
    >
      <slot />
    </ContextMenuTrigger>
    <ContextMenuPortal>
      <ContextMenuContent
        class="ui-menu"
        :style="
          minWidth === undefined ? undefined : { minWidth: `${minWidth}px` }
        "
        @escape-key-down="handleEscape"
        @close-auto-focus="handleCloseAutoFocus"
      >
        <ContextMenuItem
          v-for="(item, index) in resolveItems()"
          :key="`${item.label}:${index}`"
          class="ui-menu-item"
          :aria-label="item.label"
          :disabled="item.disabled || undefined"
          @select="() => runItem(item)"
        >
          <span>{{ item.label }}</span>
          <kbd
            v-if="item.shortcut"
            class="ui-menu-shortcut"
            >{{ formatShortcut(item.shortcut, platform) }}</kbd
          >
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenuPortal>
  </ContextMenuRoot>
</template>

<script setup lang="ts">
import {
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuPortal,
  ContextMenuRoot,
  ContextMenuTrigger,
} from 'reka-ui';
import { ref } from 'vue';

import {
  formatShortcut,
  type ShortcutPlatform,
} from '../../lib/app-commands/shortcut';

import type { UiMenuItem } from './UiMenu.vue';

const props = withDefaults(
  defineProps<{
    items: UiMenuItem[] | (() => UiMenuItem[]);
    minWidth?: number;
  }>(),
  { minWidth: 152 },
);

const trigger = ref<HTMLElement | null>(null);
const restoreFocusOnClose = ref(false);
const focusable =
  'button:not(:disabled), input:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

function rememberTrigger(event: MouseEvent): void {
  trigger.value =
    event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
  restoreFocusOnClose.value = false;
}

function handleEscape(): void {
  restoreFocusOnClose.value = true;
}

function handleCloseAutoFocus(event: Event): void {
  if (!restoreFocusOnClose.value) return;
  restoreFocusOnClose.value = false;
  const origin = trigger.value;
  if (!origin?.isConnected) return;
  const target = origin.matches(focusable)
    ? origin
    : origin.querySelector<HTMLElement>(focusable);
  if (!target) return;
  event.preventDefault();
  target.focus({ preventScroll: true });
}

const platform: ShortcutPlatform = /Mac|iPhone|iPad/.test(navigator.platform)
  ? 'mac'
  : 'non-mac';

function resolveItems(): UiMenuItem[] {
  return typeof props.items === 'function' ? props.items() : props.items;
}

function runItem(item: UiMenuItem): void {
  item.run?.();
}
</script>

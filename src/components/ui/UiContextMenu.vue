<template>
  <!--
    The surface and the item are styled in ui/surface.css: reka portals this
    content to the body, where a scoped attribute never reaches it, and the two
    menus used to answer that by carrying a copy of the stylesheet each.
  -->
  <ContextMenuRoot>
    <ContextMenuTrigger as-child>
      <slot />
    </ContextMenuTrigger>
    <ContextMenuPortal>
      <ContextMenuContent
        class="ui-menu"
        :style="
          minWidth === undefined ? undefined : { minWidth: `${minWidth}px` }
        "
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

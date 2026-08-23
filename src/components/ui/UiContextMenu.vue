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
          :disabled="item.disabled || undefined"
          @select="() => runItem(item)"
        >
          {{ item.label }}
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

import type { UiMenuItem } from './UiMenu.vue';

const props = withDefaults(
  defineProps<{
    items: UiMenuItem[] | (() => UiMenuItem[]);
    minWidth?: number;
  }>(),
  { minWidth: 152 },
);

/**
 * A function items source is evaluated at render time, so a menu that opens
 * over a changing surface (a text field's selection, say) reads it fresh.
 */
function resolveItems(): UiMenuItem[] {
  return typeof props.items === 'function' ? props.items() : props.items;
}

function runItem(item: UiMenuItem): void {
  item.run?.();
}
</script>

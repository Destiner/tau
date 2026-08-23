<template>
  <!--
    The surface and the item are styled in ui/surface.css: reka portals this
    content to the body, where a scoped attribute never reaches it, and the two
    menus used to answer that by carrying a copy of the stylesheet each.
  -->
  <DropdownMenuRoot v-model:open="open">
    <DropdownMenuTrigger as-child>
      <slot name="trigger" />
    </DropdownMenuTrigger>
    <DropdownMenuPortal>
      <DropdownMenuContent
        class="ui-menu"
        :style="
          minWidth === undefined ? undefined : { minWidth: `${minWidth}px` }
        "
        :side="side"
        :align="align"
        :side-offset="sideOffset"
      >
        <DropdownMenuItem
          v-for="(item, index) in resolveItems()"
          :key="`${item.label}:${index}`"
          class="ui-menu-item"
          :disabled="item.disabled || undefined"
          @select="() => runItem(item)"
        >
          {{ item.label }}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenuPortal>
  </DropdownMenuRoot>
</template>

<script setup lang="ts">
import {
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuPortal,
  DropdownMenuRoot,
  DropdownMenuTrigger,
} from 'reka-ui';

const open = defineModel<boolean>('open', { default: false });

const props = withDefaults(
  defineProps<{
    items: UiMenuItem[] | (() => UiMenuItem[]);
    minWidth?: number;
    side?: 'top' | 'right' | 'bottom' | 'left';
    align?: 'start' | 'center' | 'end';
    sideOffset?: number;
  }>(),
  {
    minWidth: 152,
    side: 'top',
    align: 'start',
    sideOffset: 5,
  },
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

export interface UiMenuItem {
  label: string;
  disabled?: boolean;
  run?: () => void;
}
</script>

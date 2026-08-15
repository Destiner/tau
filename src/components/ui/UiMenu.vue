<template>
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

<style scoped>
/* reka portals the content to the body, which drops the scoped attribute,
 * so the menu is styled through :deep on its namespaced classes. */
:global(.ui-menu) {
  padding: 4px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel-raised);
  box-shadow: 0 8px 24px var(--shadow-soft);
}

:global(.ui-menu-item) {
  display: block;
  width: 100%;
  padding: 7px 8px;
  border-radius: 5px;
  background: transparent;
  color: var(--text);
  font-size: 12px;
  text-align: left;
}

:global(.ui-menu-item:not([data-disabled]):hover),
:global(.ui-menu-item:not([data-disabled])[data-highlighted]),
:global(.ui-menu-item:not([data-disabled]):focus-visible) {
  outline: 0;
  background: var(--hover);
}

:global(.ui-menu-item[data-disabled]) {
  color: var(--faint);
}
</style>

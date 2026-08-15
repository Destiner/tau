<template>
  <TooltipProvider :delay-duration="delayDuration">
    <TooltipRoot>
      <TooltipTrigger as-child>
        <slot />
      </TooltipTrigger>
      <TooltipPortal>
        <TooltipContent
          class="ui-tooltip"
          :side="side"
          :align="align"
          :side-offset="sideOffset"
        >
          {{ content }}
        </TooltipContent>
      </TooltipPortal>
    </TooltipRoot>
  </TooltipProvider>
</template>

<script setup lang="ts">
import {
  TooltipContent,
  TooltipPortal,
  TooltipProvider,
  TooltipRoot,
  TooltipTrigger,
} from 'reka-ui';

withDefaults(
  defineProps<{
    content: string;
    side?: 'top' | 'right' | 'bottom' | 'left';
    align?: 'start' | 'center' | 'end';
    sideOffset?: number;
    delayDuration?: number;
  }>(),
  {
    side: 'top',
    align: 'center',
    sideOffset: 6,
    delayDuration: 700,
  },
);
</script>

<style scoped>
/* reka portals the tooltip to the body, where the scoped attribute does not
 * reach it, so it is styled with :global on its namespaced class. */
:global(.ui-tooltip) {
  max-width: 260px;
  padding: 4px 7px;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: var(--panel-raised);
  box-shadow: 0 4px 14px var(--shadow-soft);
  color: var(--text);
  font-size: 11px;
  line-height: 1.4;
}
</style>

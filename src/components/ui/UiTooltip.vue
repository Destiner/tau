<template>
  <!--
    The provider lives here rather than at the app root so that a tooltip works
    wherever it is mounted — the app, a dev fixture, a test — without a second
    thing to remember. Nesting providers is allowed; the cost is that the
    "slide to the neighbouring button and see it at once" skip window is per
    tooltip rather than shared, which is invisible at this delay.

    One rule for callers: a trigger that opens its own floating surface (a menu,
    a popover) must not be wrapped directly. `TooltipRoot` is a popper root, and
    the nearest popper root is the one a trigger registers itself with, so a
    menu trigger placed in this slot hands the tooltip its anchor and leaves its
    own menu to be positioned from nothing. Put its own root in between — wrap a
    plain element around the whole `UiMenu`/`PopoverRoot` and label that instead.
    Doing so costs the keyboard reveal, since focus does not bubble to the
    wrapper; the pointer, which is who a tooltip is for, is unaffected.
  -->
  <TooltipProvider :delay-duration="delay">
    <TooltipRoot>
      <TooltipTrigger as-child>
        <slot />
      </TooltipTrigger>
      <TooltipPortal>
        <!-- Styled in ui/surface.css: reka portals this to the body, where a
             scoped attribute never reaches it. -->
        <!-- Kept off the window edge by the same gap it keeps from its
             trigger, for a long one that has to shift to stay on screen. -->
        <TooltipContent
          class="ui-tooltip"
          :side="side"
          :side-offset="6"
          :collision-padding="6"
        >
          {{ text }}
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
    /**
     * The name of the thing the trigger does — an icon button's label, said
     * out loud — or the value a row shows too little of, such as a project's
     * path. A reviewed status reason may be a sentence; never show raw error
     * payloads or shortcut hints here.
     */
    text: string;
    side?: 'top' | 'right' | 'bottom' | 'left';
    delay?: number;
  }>(),
  { side: 'top', delay: 450 },
);
</script>

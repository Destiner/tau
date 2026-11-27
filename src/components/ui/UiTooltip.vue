<template>
  <TooltipProvider
    v-if="$slots.default || visible"
    :delay-duration="delay"
  >
    <TooltipRoot
      :disabled="disabled"
      @update:open="handleSlotOpen"
    >
      <TooltipTrigger
        v-if="$slots.default"
        as-child
      >
        <slot />
      </TooltipTrigger>
      <component
        :is="$slots.default ? TooltipPortal : Teleport"
        to="body"
      >
        <component
          :is="$slots.default ? TooltipContent : 'div'"
          :ref="setContent"
          class="ui-tooltip"
          :class="[
            $slots.default ? undefined : 'ui-tooltip-anchored',
            contentClass,
          ]"
          v-bind="$slots.default ? rekaPlacement : externalPlacement"
        >
          <template v-if="shortcut">
            <span class="ui-tooltip-label">
              <slot name="content">{{ text }}</slot>
            </span>
            <kbd class="ui-tooltip-shortcut">{{ formattedShortcut }}</kbd>
          </template>
          <slot
            v-else
            name="content"
            >{{ text }}</slot
          >
        </component>
      </component>
    </TooltipRoot>
  </TooltipProvider>
</template>

<script setup lang="ts">
import {
  autoUpdate,
  computePosition,
  flip,
  offset,
  shift,
} from '@floating-ui/dom';
import {
  TooltipContent,
  TooltipPortal,
  TooltipProvider,
  TooltipRoot,
  TooltipTrigger,
} from 'reka-ui';
import {
  type ComponentPublicInstance,
  computed,
  shallowRef,
  Teleport,
  watch,
} from 'vue';

import {
  formatShortcut,
  type ShortcutInput,
  type ShortcutPlatform,
} from '../../lib/app-commands/shortcut';

import { activeActionTooltipAnchor } from './action-tooltip';

const props = withDefaults(
  defineProps<{
    text: string;
    /** A real keyboard binding for this action, never an invented hint. */
    shortcut?: ShortcutInput;
    side?: 'top' | 'right' | 'bottom' | 'left';
    align?: 'start' | 'center' | 'end';
    delay?: number;
    contentClass?: string;
    anchor?: HTMLElement;
    disabled?: boolean;
    /** Set only when the owning surface delegates events to a shared anchor. */
    open?: boolean;
  }>(),
  {
    shortcut: undefined,
    side: 'top',
    align: 'center',
    delay: 450,
    contentClass: undefined,
    anchor: undefined,
    open: undefined,
  },
);

const emit = defineEmits<{ 'update:open': [open: boolean] }>();

const gap = 6;
const content = shallowRef<HTMLElement>();
const visible = computed(() =>
  Boolean(
    props.anchor &&
    !props.disabled &&
    (props.open ?? activeActionTooltipAnchor.value === props.anchor),
  ),
);
const rekaPlacement = computed(() => ({
  side: props.side,
  align: props.align,
  sideOffset: gap,
  collisionPadding: gap,
}));
const externalPlacement = { role: 'tooltip' };

function setContent(element: Element | ComponentPublicInstance | null): void {
  content.value = element instanceof HTMLElement ? element : undefined;
}

function handleSlotOpen(open: boolean): void {
  if (open) activeActionTooltipAnchor.value = undefined;
}

watch(
  () => [props.anchor, props.disabled, props.open !== undefined] as const,
  ([anchor, disabled, controlled], _, onCleanup) => {
    if (!anchor || disabled || controlled) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const close = (): void => {
      clearTimeout(timer);
      if (activeActionTooltipAnchor.value === anchor)
        activeActionTooltipAnchor.value = undefined;
    };
    const hover = (event: PointerEvent): void => {
      if (event.pointerType === 'touch' || event.buttons) return;
      timer = setTimeout(() => {
        activeActionTooltipAnchor.value = anchor;
      }, props.delay);
    };
    const focus = (): void => {
      if (!anchor.matches(':focus-visible')) return;
      clearTimeout(timer);
      activeActionTooltipAnchor.value = anchor;
    };
    const listeners = new AbortController();
    const options = { signal: listeners.signal };
    anchor.addEventListener('pointerenter', hover, options);
    anchor.addEventListener('pointerleave', close, options);
    anchor.addEventListener('pointerdown', close, options);
    anchor.addEventListener('click', close, options);
    anchor.addEventListener('focus', focus, options);
    anchor.addEventListener('blur', close, options);
    onCleanup(() => {
      close();
      listeners.abort();
    });
  },
  { flush: 'post', immediate: true },
);

watch(
  () => [props.anchor, content.value, props.side, props.align] as const,
  ([anchor, element, side, align], _, onCleanup) => {
    if (!anchor || !element) return;
    let active = true;
    // External hints share placement with Reka's hints but are not dismissible
    // layers: a hint must not take Escape away from its dialog or popover.
    const stop = autoUpdate(anchor, element, () => {
      void computePosition(anchor, element, {
        strategy: 'fixed',
        placement: align === 'center' ? side : `${side}-${align}`,
        middleware: [
          offset(gap),
          flip({ padding: gap }),
          shift({ padding: gap }),
        ],
      }).then(({ x, y }) => {
        if (!active) return;
        element.style.transform = `translate(${x}px, ${y}px)`;
        element.style.visibility = 'visible';
      });
    });
    const dismiss = (): void => {
      if (props.open !== undefined) emit('update:open', false);
      if (activeActionTooltipAnchor.value === anchor)
        activeActionTooltipAnchor.value = undefined;
    };
    const onKeydown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') dismiss();
    };
    const onScroll = (event: Event): void => {
      if (event.target instanceof Node && event.target.contains(anchor))
        dismiss();
    };
    window.addEventListener('keydown', onKeydown, true);
    window.addEventListener('scroll', onScroll, true);
    onCleanup(() => {
      active = false;
      stop();
      window.removeEventListener('keydown', onKeydown, true);
      window.removeEventListener('scroll', onScroll, true);
    });
  },
  { flush: 'post' },
);

const platform: ShortcutPlatform = /Mac|iPhone|iPad/.test(navigator.platform)
  ? 'mac'
  : 'non-mac';
const formattedShortcut = computed(() =>
  props.shortcut ? formatShortcut(props.shortcut, platform) : '',
);
</script>

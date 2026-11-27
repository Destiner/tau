<template>
  <TooltipProvider
    v-if="$slots.default"
    :delay-duration="delay"
  >
    <TooltipRoot
      :disabled="disabled"
      :open="active"
      @update:open="setOpen"
    >
      <TooltipTrigger as-child>
        <slot />
      </TooltipTrigger>
      <TooltipPortal>
        <TooltipContent
          class="ui-tooltip"
          :class="contentClass"
          :side="side"
          :align="align"
          :side-offset="gap"
          :collision-padding="gap"
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
        </TooltipContent>
      </TooltipPortal>
    </TooltipRoot>
  </TooltipProvider>
  <Teleport
    v-else-if="anchor && active"
    to="body"
  >
    <div
      :id="id"
      ref="content"
      class="ui-tooltip ui-tooltip-anchored"
      :class="contentClass"
      role="tooltip"
    >
      <template v-if="shortcut">
        <span class="ui-tooltip-label">{{ text }}</span>
        <kbd class="ui-tooltip-shortcut">{{ formattedShortcut }}</kbd>
      </template>
      <template v-else>{{ text }}</template>
    </div>
  </Teleport>
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
import { computed, onBeforeUnmount, shallowRef, useId, watch } from 'vue';

import {
  formatShortcut,
  type ShortcutInput,
  type ShortcutPlatform,
} from '../../lib/app-commands/shortcut';

import { ACTION_TOOLTIP_DELAY, activeTooltipId } from './action-tooltip';

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
    /** The owning surface mounts the hint in response to delegated events. */
    delegated?: boolean;
  }>(),
  {
    shortcut: undefined,
    side: 'top',
    align: 'center',
    delay: ACTION_TOOLTIP_DELAY,
    contentClass: undefined,
    anchor: undefined,
  },
);

const emit = defineEmits<{ dismiss: [] }>();

const gap = 6;
const id = `tau-tooltip-${useId()}`;
const content = shallowRef<HTMLDivElement | null>(null);
const active = computed(() => activeTooltipId.value === id);

function setOpen(open: boolean): void {
  if (open) activeTooltipId.value = id;
  else if (active.value) activeTooltipId.value = undefined;
}

onBeforeUnmount(() => setOpen(false));

watch(
  () => [props.anchor, props.disabled, props.delegated] as const,
  ([anchor, disabled, delegated], _, onCleanup) => {
    if (!anchor || disabled) return;
    if (delegated) {
      setOpen(true);
      onCleanup(() => setOpen(false));
      return;
    }

    let timer: ReturnType<typeof setTimeout> | undefined;
    const close = (): void => {
      clearTimeout(timer);
      setOpen(false);
    };
    const hover = (event: PointerEvent): void => {
      if (event.pointerType === 'touch' || event.buttons) return;
      timer = setTimeout(() => setOpen(true), props.delay);
    };
    const focus = (): void => {
      if (!anchor.matches(':focus-visible')) return;
      clearTimeout(timer);
      setOpen(true);
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
    updateDescription(anchor, true);
    const listeners = new AbortController();
    const { signal } = listeners;
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
        if (signal.aborted) return;
        element.style.transform = `translate(${x}px, ${y}px)`;
        element.style.visibility = 'visible';
      });
    });
    const dismiss = (): void => {
      setOpen(false);
      if (props.delegated) emit('dismiss');
    };
    const onKeydown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') dismiss();
    };
    const onScroll = (event: Event): void => {
      if (event.target instanceof Node && event.target.contains(anchor))
        dismiss();
    };
    const options = { capture: true, signal };
    window.addEventListener('keydown', onKeydown, options);
    window.addEventListener('scroll', onScroll, options);
    onCleanup(() => {
      listeners.abort();
      updateDescription(anchor, false);
      stop();
    });
  },
  { flush: 'post' },
);

function updateDescription(anchor: HTMLElement, open: boolean): void {
  const attribute = 'aria-describedby';
  const descriptions = new Set(anchor.getAttribute(attribute)?.match(/\S+/g));
  if (open) descriptions.add(id);
  else descriptions.delete(id);
  if (descriptions.size)
    anchor.setAttribute(attribute, [...descriptions].join(' '));
  else anchor.removeAttribute(attribute);
}

const platform: ShortcutPlatform = /Mac|iPhone|iPad/.test(navigator.platform)
  ? 'mac'
  : 'non-mac';
const formattedShortcut = computed(() =>
  props.shortcut ? formatShortcut(props.shortcut, platform) : '',
);
</script>

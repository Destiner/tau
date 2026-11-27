<template>
  <TooltipProvider
    v-if="$slots.default || visible"
    :delay-duration="delay"
  >
    <TooltipRoot
      :disabled="disabled"
      :open="Boolean($slots.default && active)"
      @update:open="setOpen"
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
  onBeforeUnmount,
  shallowRef,
  Teleport,
  useId,
  watch,
} from 'vue';

import {
  formatShortcut,
  type ShortcutInput,
  type ShortcutPlatform,
} from '../../lib/app-commands/shortcut';

import { activeTooltipId } from './action-tooltip';

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
const id = `tau-tooltip-${useId()}`;
const content = shallowRef<HTMLElement>();
const active = computed(() => activeTooltipId.value === id);
const visible = computed(() => Boolean(props.anchor && active.value));
const rekaPlacement = computed(() => ({
  side: props.side,
  align: props.align,
  sideOffset: gap,
  collisionPadding: gap,
}));
const externalPlacement = { id, role: 'tooltip' };

function setContent(element: Element | ComponentPublicInstance | null): void {
  content.value = element instanceof HTMLElement ? element : undefined;
}

function setOpen(open: boolean): void {
  if (open) activeTooltipId.value = id;
  else if (active.value) activeTooltipId.value = undefined;
}

onBeforeUnmount(() => setOpen(false));

watch(
  () => [props.anchor, props.disabled, props.open] as const,
  ([anchor, disabled, requestedOpen], _, onCleanup) => {
    if (!anchor || disabled) return;
    if (requestedOpen !== undefined) {
      setOpen(requestedOpen);
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
    let positioning = true;
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
        if (!positioning) return;
        element.style.transform = `translate(${x}px, ${y}px)`;
        element.style.visibility = 'visible';
      });
    });
    const dismiss = (): void => {
      setOpen(false);
      if (props.open !== undefined) emit('update:open', false);
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
      positioning = false;
      updateDescription(anchor, false);
      stop();
      window.removeEventListener('keydown', onKeydown, true);
      window.removeEventListener('scroll', onScroll, true);
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

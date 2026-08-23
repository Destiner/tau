<template>
  <input
    ref="input"
    class="ui-input"
    :data-error="error || undefined"
    :data-variant="variant"
    :value="modelValue"
    v-bind="$attrs"
    @input="handleInput"
  />
</template>

<script setup lang="ts">
import { ref } from 'vue';

const modelValue = defineModel<string>({ default: '' });

withDefaults(
  defineProps<{
    /** `bordered` field, `mono` connection field, or borderless inline text. */
    variant?: 'bordered' | 'mono' | 'bare';
    error?: boolean;
  }>(),
  { variant: 'bordered' },
);

defineOptions({ inheritAttrs: false });

const input = ref<HTMLInputElement>();

function handleInput(event: Event): void {
  modelValue.value = (event.target as HTMLInputElement).value;
}

/** The underlying element, for the imperative focus/select callers need. */
defineExpose({
  get input() {
    return input.value;
  },
});
</script>

<style scoped>
/* stylelint-disable no-descending-specificity -- state rules are ordered by which should win: hover, focus, then error. */

.ui-input {
  width: 100%;
  border-radius: var(--radius-sm);
  outline: 2px solid transparent;
  background: var(--canvas);
  color: var(--text);
  line-height: var(--leading-ui);
}

.ui-input::placeholder {
  color: var(--faint);
}

.ui-input:read-only {
  color: var(--muted);
}

.ui-input[data-variant='bordered'] {
  height: var(--control-lg);
  padding: 0 8px;
  border: 1px solid var(--border);
  font-size: var(--text-sm);
}

.ui-input[data-variant='mono'] {
  height: var(--control-lg);
  padding: 0 8px;
  border: 1px solid var(--border);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-sm);
}

/*
 * `bare` is inline text that happens to be editable — an inline-renamed
 * session title. It gains no border, no hover and no focus ring: the caret is
 * the only thing that should say it is being edited.
 */
.ui-input[data-variant='bare'] {
  display: block;
  width: 100%;
  max-width: 60ch;
  height: auto;
  padding: 2px 5px;
  border: 0;
  background: transparent;
  font-size: var(--text-md);
}

.ui-input[data-variant='bordered']:hover:not(
    :disabled,
    :focus,
    :read-only,
    [data-error]
  ),
.ui-input[data-variant='mono']:hover:not(
    :disabled,
    :focus,
    :read-only,
    [data-error]
  ) {
  border-color: color-mix(in srgb, var(--muted) 45%, var(--border));
}

.ui-input[data-variant='bordered']:focus,
.ui-input[data-variant='mono']:focus {
  border-color: var(--accent);
  outline: 0;
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 22%, transparent);
}

/*
 * A rejected value outranks both the pointer and the caret, and it has to be
 * declared after the variants: those set the `border` shorthand, which used to
 * overwrite the danger color and left `error` doing nothing on a bordered or
 * mono field — the connection field being the one place it matters most.
 */
.ui-input[data-error] {
  border-color: var(--danger);
}

.ui-input[data-error]:hover:not(:disabled) {
  border-color: var(--danger);
  background: color-mix(in srgb, var(--danger) 5%, var(--canvas));
}

.ui-input[data-error]:focus {
  border-color: var(--danger);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--danger) 20%, transparent);
}
</style>

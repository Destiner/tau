<template>
  <textarea
    ref="textarea"
    class="ui-textarea"
    :data-variant="variant"
    :value="modelValue"
    v-bind="$attrs"
    @input="handleInput"
  ></textarea>
</template>

<script setup lang="ts">
import { ref } from 'vue';

const modelValue = defineModel<string>({ default: '' });

withDefaults(
  defineProps<{
    variant?: 'bordered' | 'bare';
  }>(),
  { variant: 'bordered' },
);

defineOptions({ inheritAttrs: false });

const textarea = ref<HTMLTextAreaElement>();

function handleInput(event: Event): void {
  modelValue.value = (event.target as HTMLTextAreaElement).value;
}

/** The underlying element, for the imperative focus/select callers need. */
defineExpose({
  get input() {
    return textarea.value;
  },
});
</script>

<style scoped>
/* stylelint-disable no-descending-specificity -- state rules are ordered by which should win: hover, then focus. */

.ui-textarea {
  width: 100%;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  outline: 2px solid transparent;
  background: var(--canvas);
  color: var(--text);
  font-size: var(--text-sm);
  line-height: var(--leading-ui);
}

.ui-textarea::placeholder {
  color: var(--faint);
}

.ui-textarea[data-variant='bordered'] {
  min-height: 120px;
  max-height: min(300px, 35vh);
  border: 1px solid var(--border);
  resize: vertical;
}

/*
 * `bare` is a body of text with no chrome of its own — the composer, which is
 * the field the app is mostly used through. It gains no border, no hover and no
 * focus ring: the surface around it already says where typing goes.
 */
.ui-textarea[data-variant='bare'] {
  border: 0;
  background: transparent;
}

.ui-textarea[data-variant='bordered']:hover:not(:disabled, :focus, :read-only) {
  border-color: color-mix(in srgb, var(--muted) 45%, var(--border));
}

.ui-textarea[data-variant='bordered']:focus {
  border-color: var(--accent);
  outline: 0;
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 22%, transparent);
}
</style>

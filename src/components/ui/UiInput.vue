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
.ui-input {
  width: 100%;
  border-radius: 5px;
  outline: 0;
  background: var(--canvas);
  color: var(--text);
  line-height: 1.45;
}

.ui-input:focus {
  border-color: var(--accent);
}

.ui-input::placeholder {
  color: var(--faint);
}

.ui-input[data-error] {
  border-color: var(--danger);
}

.ui-input:read-only {
  color: var(--muted);
}

.ui-input[data-variant='bordered'] {
  height: 30px;
  padding: 5px 7px;
  border: 1px solid var(--border);
  font-size: 12px;
}

.ui-input[data-variant='mono'] {
  height: 34px;
  padding: 0 9px;
  border: 1px solid var(--border);
  border-radius: 6px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
}

.ui-input[data-variant='bare'] {
  display: block;
  width: 100%;
  max-width: 60ch;
  padding: 2px 5px;
  border: 0;
  background: transparent;
  font-size: 13px;
}
</style>

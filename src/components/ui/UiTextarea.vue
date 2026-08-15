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
.ui-textarea {
  width: 100%;
  padding: 5px 7px;
  border: 1px solid var(--border);
  border-radius: 5px;
  outline: 0;
  background: var(--canvas);
  color: var(--text);
  font-size: 12px;
  line-height: 1.45;
}

.ui-textarea:focus {
  border-color: var(--accent);
}

.ui-textarea::placeholder {
  color: var(--faint);
}

.ui-textarea[data-variant='bordered'] {
  min-height: 120px;
  max-height: min(300px, 35vh);
  resize: vertical;
}

.ui-textarea[data-variant='bare'] {
  border: 0;
  background: transparent;
}
</style>

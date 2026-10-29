<template>
  <span class="ui-token-field">
    <UiTextarea
      ref="field"
      :model-value="modelValue"
      :rows="1"
      wrap="off"
      autocomplete="off"
      autocorrect="off"
      autocapitalize="none"
      :spellcheck="false"
      v-bind="$attrs"
      @update:model-value="updateValue"
      @scroll="syncScroll"
      @keydown="handleKeydown"
    />
    <span
      class="ui-token-mask"
      aria-hidden="true"
    >
      <span :style="{ transform: `translateX(${-scrollLeft}px)` }">{{
        '•'.repeat(modelValue.length)
      }}</span>
    </span>
  </span>
</template>

<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';

import UiTextarea from './UiTextarea.vue';

const modelValue = defineModel<string>({ default: '' });
defineOptions({ inheritAttrs: false });
const field = ref<InstanceType<typeof UiTextarea>>();
const scrollLeft = ref(0);

function syncScroll(): void {
  scrollLeft.value = field.value?.input?.scrollLeft ?? 0;
}
function updateValue(value: string): void {
  modelValue.value = value.replace(/[\r\n]/g, '');
  void nextTick(() => {
    if (field.value?.input) field.value.input.value = modelValue.value;
    syncScroll();
  });
}
function handleKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Enter' || event.isComposing) return;
  event.preventDefault();
  if (!event.metaKey && !event.ctrlKey)
    field.value?.input?.form?.requestSubmit();
}
watch(modelValue, () => void nextTick(syncScroll));
defineExpose({
  get input() {
    return field.value?.input;
  },
});
</script>

<style scoped>
.ui-token-field {
  display: block;
  position: relative;
  min-width: 0;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-sm);
}

/* API tokens are not login passwords; native password hints invoke Safari AutoFill. */
.ui-token-field :deep(.ui-textarea[data-variant='bordered']) {
  display: block;
  height: var(--control-lg);
  min-height: var(--control-lg);
  max-height: var(--control-lg);
  padding: 0 8px;
  overflow: hidden;
  color: transparent;
  caret-color: var(--text);
  font: inherit;
  line-height: calc(var(--control-lg) - 2px);
  -webkit-text-fill-color: transparent;
}

.ui-token-field :deep(.ui-textarea::placeholder) {
  -webkit-text-fill-color: var(--faint);
}

.ui-token-mask {
  display: flex;
  position: absolute;
  align-items: center;
  overflow: hidden;
  color: var(--text);
  line-height: calc(var(--control-lg) - 2px);
  pointer-events: none;
  inset: 1px 9px;
}

.ui-token-mask span {
  display: block;
  flex: none;
  white-space: pre;
}
</style>

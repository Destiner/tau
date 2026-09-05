<template>
  <main class="model-selector-fixture">
    <section>
      <ModelSelector
        v-model="selectedModel"
        :models="models"
        aria-label="Model"
      />
      <button type="button">Outside</button>
    </section>
    <section>
      <ModelSelector
        v-model="emptyModel"
        :models="[]"
        aria-label="Empty Model"
      />
    </section>
  </main>
</template>

<script setup lang="ts">
import { ref } from 'vue';

import ModelSelector from '../components/ModelSelector.vue';
import type { ModelOption } from '../lib/pi/model-scope';

const selectedModel = ref('openai-codex/gpt-5.6-sol');
const emptyModel = ref('');
const models: ModelOption[] = [
  {
    provider: 'openai-codex',
    id: 'gpt-5.6-sol',
    name: 'GPT-5.6 Sol',
    reasoning: true,
  },
  {
    provider: 'openrouter',
    id: 'deepseek/deepseek-v4-flash-0731',
    name: 'DeepSeek V4 Flash',
    reasoning: true,
  },
  {
    provider: 'pi-claude',
    id: 'claude-fable-5',
    name: 'Claude Fable 5',
    reasoning: true,
  },
  {
    provider: 'pi-claude',
    id: 'claude-opus-5',
    name: 'Claude Opus 5',
    reasoning: true,
  },
  {
    provider: 'openai-codex',
    id: 'gpt-6-astra',
    name: 'GPT-6 Astra',
    reasoning: true,
  },
];
</script>

<style scoped>
.model-selector-fixture {
  display: flex;
  align-items: flex-end;
  justify-content: center;
  min-height: 100%;
  padding: 80px;
  background: var(--canvas);
  gap: 80px;
}

.model-selector-fixture section {
  display: flex;
  align-items: center;
  gap: 12px;
}
</style>

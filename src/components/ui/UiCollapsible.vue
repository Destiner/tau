<template>
  <CollapsibleRoot
    v-model:open="open"
    :disabled="disabled || undefined"
  >
    <CollapsibleTrigger as-child>
      <slot
        name="trigger"
        :open="open"
      />
    </CollapsibleTrigger>
    <CollapsibleContent>
      <slot name="content" />
    </CollapsibleContent>
  </CollapsibleRoot>
</template>

<script setup lang="ts">
import {
  CollapsibleContent,
  CollapsibleRoot,
  CollapsibleTrigger,
} from 'reka-ui';

const open = defineModel<boolean>('open', { default: false });

withDefaults(
  defineProps<{
    disabled?: boolean;
  }>(),
  { disabled: undefined },
);
</script>

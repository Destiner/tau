<template>
  <span
    class="ui-status-dot"
    :class="tone ? `tone-${tone}` : undefined"
    :role="label ? 'img' : undefined"
    :aria-label="label || undefined"
    :aria-hidden="label ? undefined : true"
    :title="label || undefined"
  ></span>
</template>

<script setup lang="ts">
withDefaults(
  defineProps<{
    /**
     * The state the dot carries; without one the dot stays invisible and only
     * reserves its slot.
     */
    tone?: 'new' | 'draft' | 'working';
    /** Meaning announced to assistive tech, and the title a pointer sees. */
    label?: string;
  }>(),
  { label: undefined, tone: undefined },
);
</script>

<style scoped>
/*
 * It sits on the text baseline rather than in the middle of the row: a flex
 * item with no content takes its bottom edge as its baseline. At 6px, ending
 * on that baseline optically centres the mark against the lowercase title.
 */
.ui-status-dot {
  flex: none;
  align-self: baseline;
  width: 6px;
  height: 6px;
  margin-bottom: 0;
  border-radius: 50%;
  opacity: 0;
}

.ui-status-dot.tone-new {
  opacity: 1;
  background: var(--status-new);
}

.ui-status-dot.tone-draft {
  opacity: 0.3;
  background: var(--status-working);
}

.ui-status-dot.tone-working {
  opacity: 1;
  background: var(--status-working);
}
</style>

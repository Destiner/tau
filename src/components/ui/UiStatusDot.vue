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
 * 7px, and it sits on the text baseline rather than in the middle of the row:
 * a flex item with no content takes its bottom edge as its baseline, so in a
 * baseline-aligned row the dot rests on the label's baseline, and the -1px
 * sinks it the hair that makes it read as centred against lowercase letters.
 */
.ui-status-dot {
  flex: none;
  align-self: baseline;
  width: 7px;
  height: 7px;
  margin-bottom: -1px;
  border-radius: 50%;
  opacity: 0;
}

.ui-status-dot.tone-new {
  opacity: 1;
  background: var(--status-new);
}

.ui-status-dot.tone-draft {
  border: 1.5px solid var(--status-draft);
  opacity: 1;
}

.ui-status-dot.tone-working {
  opacity: 1;
  background: var(--status-working);
}
</style>

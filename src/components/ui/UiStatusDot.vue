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
 * item with no content takes its border box's bottom edge as its baseline. A
 * mark resting exactly there reads a hair low against lowercase titles, so it
 * lifts three quarters of a pixel off the baseline onto their optical centre.
 *
 * The lift has to be a transform. Margin cannot move the mark at all, because
 * the synthesized baseline comes from the border box and ignores it, and a
 * relative offset rounds the sub-pixel away: up to a whole pixel in Chromium,
 * down to nothing in WebKit at 1x. Rows that center the mark instead of
 * aligning it to a baseline override --status-dot-nudge; positive moves down.
 */
.ui-status-dot {
  flex: none;
  align-self: baseline;
  width: 6px;
  height: 6px;
  transform: translateY(var(--status-dot-nudge, -0.75px));
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

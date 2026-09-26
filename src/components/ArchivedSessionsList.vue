<template>
  <div
    ref="scrollElement"
    class="archived-list"
    @scroll="scheduleAppend"
  >
    <template
      v-for="group in windowed.groups"
      :key="group.label"
    >
      <button
        class="group-head"
        type="button"
        :aria-expanded="!closedGroups.has(group.label)"
        @click="() => toggleGroup(group.label)"
      >
        <span>{{ group.label }}</span>
        <UiIcon
          :name="
            closedGroups.has(group.label) ? 'chevron-right' : 'chevron-down'
          "
        />
      </button>
      <template v-if="!closedGroups.has(group.label)">
        <div
          v-for="entry in group.items"
          :key="`${entry.projectPath}:${entry.session.id}`"
          class="row"
          :class="{
            selected:
              entry.projectPath === state.activeProjectPath &&
              entry.session.id === state.activeSessionId,
          }"
        >
          <button
            class="copy"
            type="button"
            :aria-label="`Open ${entry.session.title}`"
            :disabled="projectActionsDisabled"
            @click="() => open(entry)"
          >
            <span class="name">{{ entry.session.title }}</span>
            <span class="meta">
              <span class="project">{{ entry.projectName }}</span>
              <template v-if="entry.session.model">
                <span class="dot">·</span>
                <span class="model">{{ entry.session.model }}</span>
              </template>
            </span>
          </button>
          <span class="time">{{ relativeTime(entry) }}</span>
          <UiTooltip text="Unarchive Session">
            <UiIconButton
              class="unarchive"
              size="md"
              variant="reveal"
              :label="`Unarchive ${entry.session.title}`"
              :disabled="projectActionsDisabled"
              @click="() => unarchive(entry)"
            >
              <UiIcon name="archive" />
            </UiIconButton>
          </UiTooltip>
        </div>
      </template>
    </template>
    <div
      v-if="groups.length === 0"
      class="empty"
    >
      No archived sessions
    </div>
    <div
      ref="sentinel"
      aria-hidden="true"
    ></div>
  </div>
</template>

<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from 'vue';

import type {
  ArchivedSessionEntry,
  ProjectSummary,
} from '../composables/state';
import useTau from '../composables/useTau';
import {
  groupArchivedByTime,
  type ArchivedGroup,
} from '../lib/archived-groups';
import archivedWindow from '../lib/archived-window';

import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiTooltip from './ui/UiTooltip.vue';

const {
  archivedSessionEntries,
  projectActionsDisabled,
  selectSession,
  sessionSortAt,
  relativeTimestamp,
  state,
  unarchiveSession,
} = useTau();

const BATCH_SIZE = 50;
const PREFETCH_PX = 200;
const closedGroups = ref(new Set<string>());
const budget = ref(BATCH_SIZE);
const scrollElement = ref<HTMLElement>();
const sentinel = ref<HTMLElement>();
let observer: IntersectionObserver | undefined;
let resizeObserver: ResizeObserver | undefined;
let frame = 0;
let disposed = false;
const groups = computed<ArchivedGroup<ArchivedSessionEntry>[]>(() =>
  groupArchivedByTime(archivedSessionEntries.value, (entry) =>
    sessionSortAt(entry.projectPath, entry.session),
  ),
);

const windowed = computed(() =>
  archivedWindow(groups.value, closedGroups.value, budget.value),
);

function scheduleAppend(): void {
  if (disposed || frame || !windowed.value.hasMore) return;
  frame = requestAnimationFrame(() => {
    frame = 0;
    const element = scrollElement.value;
    if (
      disposed ||
      !element ||
      !windowed.value.hasMore ||
      element.scrollHeight - element.scrollTop - element.clientHeight >
        PREFETCH_PX
    )
      return;
    budget.value += BATCH_SIZE;
  });
}

// Recheck after each patch: an underfilled viewport needs another batch, but
// an ordinary viewport must not consume the entire archive while idle.
watch(windowed, () => void nextTick(scheduleAppend), { flush: 'post' });

onMounted(() => {
  const element = scrollElement.value;
  const target = sentinel.value;
  if (!element || !target) return;
  observer = new IntersectionObserver(scheduleAppend, {
    root: element,
    rootMargin: `0px 0px ${PREFETCH_PX}px 0px`,
  });
  observer.observe(target);
  resizeObserver = new ResizeObserver(scheduleAppend);
  resizeObserver.observe(element);
  scheduleAppend();
});
onBeforeUnmount(() => {
  disposed = true;
  observer?.disconnect();
  resizeObserver?.disconnect();
  cancelAnimationFrame(frame);
});

function toggleGroup(label: string): void {
  const next = new Set(closedGroups.value);
  if (next.has(label)) next.delete(label);
  else next.add(label);
  closedGroups.value = next;
}

function relativeTime(entry: ArchivedSessionEntry): string {
  const timestamp = sessionSortAt(entry.projectPath, entry.session);
  return timestamp > 0
    ? relativeTimestamp(timestamp)
    : entry.session.lastActive;
}

function archivedProject(
  entry: ArchivedSessionEntry,
): ProjectSummary | undefined {
  // Every archived entry belongs to an imported project by construction;
  // the lookup only guards against a workspace swap mid-click.
  return state.workspace?.projects.find(
    (candidate) => candidate.path === entry.projectPath,
  );
}

function unarchive(entry: ArchivedSessionEntry): void {
  const project = archivedProject(entry);
  if (project) void unarchiveSession(project, entry.session);
}

/** Opening browses the transcript; only sending restores the session. */
function open(entry: ArchivedSessionEntry): void {
  const project = archivedProject(entry);
  if (project) void selectSession(project, entry.session);
}
</script>

<style scoped>
.archived-list {
  flex: 1;
  min-height: 0;
  padding: 6px;
  overflow: auto;
}

.group-head {
  display: flex;
  align-items: center;
  width: 100%;
  height: var(--control-sm);
  margin-top: 6px;
  padding: 0 5px;
  background: transparent;
  color: var(--muted);
  font-size: var(--text-xs);
  font-weight: 400;
  text-align: left;
  gap: 4px;
}

/* The expanded chevron points down; collapsed it points sideways. */
.group-head svg {
  width: 10px;
}

.row {
  display: flex;
  position: relative;
  align-items: stretch;
  min-height: var(--session-row-height, 34px);
  border-radius: var(--radius-md);
}

.row:hover {
  background: var(--hover);
}

.row.selected {
  background: var(--selected);
}

.copy {
  display: flex;
  flex: 1;
  flex-direction: column;
  justify-content: center;
  min-width: 0;
  padding: 3px 7px;
  background: transparent;
  text-align: left;
  gap: 2px;
}

.name {
  overflow: hidden;
  color: var(--text);
  font-size: var(--text-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta {
  display: flex;
  overflow: hidden;
  gap: 4px;
}

.meta > span {
  overflow: hidden;
  color: var(--muted);
  font-size: var(--text-xs);
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* The timestamp reads with the project line, not with the title above it. */
.time {
  flex: none;
  align-self: flex-end;
  padding: 0 8px 4px 6px;
  color: var(--faint);
  font-size: var(--text-xs);
}

/* The reveal button overlays the row; its box and states live in
 * UiIconButton, and the timestamp yields while it is visible. */
.unarchive {
  position: absolute;
  top: 50%;
  right: 2px;
  transform: translateY(-50%);
}

.row:hover :deep(.unarchive),
.row:focus-within :deep(.unarchive) {
  opacity: 0.65;
}

.row:hover .time,
.row:focus-within .time {
  opacity: 0;
}

.empty {
  display: flex;
  align-items: center;
  padding: 0 5px;
  color: var(--faint);
  font-size: var(--text-xs);
}
</style>

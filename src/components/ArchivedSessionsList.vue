<template>
  <div class="archived-list">
    <template
      v-for="group in groups"
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
          :key="entry.session.id"
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
          <UiIconButton
            class="unarchive"
            size="md"
            variant="reveal"
            :label="`Unarchive ${entry.session.title}`"
            title="Unarchive session"
            @click="() => unarchive(entry)"
          >
            <UiIcon name="archive" />
          </UiIconButton>
        </div>
      </template>
    </template>
    <div
      v-if="groups.length === 0"
      class="empty"
    >
      No archived sessions
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

import type {
  ArchivedSessionEntry,
  ProjectSummary,
} from '../composables/state';
import useTau from '../composables/useTau';
import {
  groupArchivedByTime,
  type ArchivedGroup,
} from '../lib/archived-groups';

import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';

const {
  archivedSessionEntries,
  selectSession,
  sessionLastUserMessageAt,
  relativeTimestamp,
  state,
  unarchiveSession,
} = useTau();

const closedGroups = ref(new Set<string>());
const groups = computed<ArchivedGroup<ArchivedSessionEntry>[]>(() =>
  groupArchivedByTime(archivedSessionEntries.value, (entry) =>
    sessionLastUserMessageAt(entry.projectPath, entry.session),
  ),
);

function toggleGroup(label: string): void {
  const next = new Set(closedGroups.value);
  if (next.has(label)) next.delete(label);
  else next.add(label);
  closedGroups.value = next;
}

function relativeTime(entry: ArchivedSessionEntry): string {
  return relativeTimestamp(
    sessionLastUserMessageAt(entry.projectPath, entry.session),
  );
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
  height: 22px;
  margin-top: 6px;
  padding: 0 5px;
  background: transparent;
  color: var(--muted);
  font-size: 11px;
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
  border-radius: 7px;
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
  font-size: 12px;
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
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.time {
  flex: none;
  align-self: flex-end;
  padding: 0 8px 5px 6px;
  color: var(--faint);
  font-size: 10px;
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
  font-size: 11px;
}
</style>

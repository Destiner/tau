<template>
  <aside class="sidebar">
    <header
      class="sidebar-titlebar"
      @mousedown="handleTitlebarMouseDown"
      @dblclick="handleTitlebarDoubleClick"
    ></header>

    <div
      ref="projectList"
      class="project-list"
    >
      <div
        v-for="project in state.workspace?.projects"
        :key="project.path"
        class="project-group"
        :data-id="project.path"
      >
        <div
          class="project-row"
          :class="{ selected: project.path === state.activeProjectPath }"
        >
          <span
            class="project-drag-handle"
            title="Drag to reorder"
            aria-hidden="true"
          >
            <UiIcon name="grip" />
          </span>
          <button
            class="project-toggle"
            type="button"
            :title="
              project.connectionString
                ? `${project.connectionString} · ${project.workingDirectory}`
                : project.workingDirectory
            "
            @click="() => toggleProject(project)"
          >
            <span>{{ project.name }}</span>
            <UiIcon
              name="chevron"
              :class="{ expanded: !project.collapsed }"
            />
            <UiStatusDot
              v-if="projectIndicator(project)"
              :tone="projectIndicator(project) || undefined"
              :label="indicatorLabel(projectIndicator(project))"
            />
          </button>
          <UiTooltip content="New session">
            <UiIconButton
              class="row-action"
              size="md"
              variant="reveal"
              :label="`New session in ${project.name}`"
              @click="() => newSession(project)"
            >
              <UiIcon name="plus" />
            </UiIconButton>
          </UiTooltip>
          <UiTooltip content="Remove project">
            <UiIconButton
              class="row-action"
              size="md"
              variant="reveal"
              tone="danger"
              :label="`Remove ${project.name}`"
              @click="() => removeProject(project)"
            >
              <UiIcon name="trash" />
            </UiIconButton>
          </UiTooltip>
        </div>

        <div
          v-if="!project.collapsed"
          class="session-list"
        >
          <UiContextMenu
            v-for="session in projectSessions(project)"
            :key="session.id"
            :items="() => sessionMenuItems(project, session)"
          >
            <div
              class="session-row"
              :class="{
                selected: isSessionSelected(project, session),
                archivable: canArchiveSession(project, session),
              }"
            >
              <button
                class="session-select"
                type="button"
                @click="() => selectSession(project, session)"
              >
                <UiStatusDot
                  :tone="sessionIndicator(project, session) || undefined"
                  :label="indicatorLabel(sessionIndicator(project, session))"
                />
                <span class="session-copy">
                  <span class="session-title">{{ session.title }}</span>
                  <span class="session-time">{{
                    sessionLastActive(project, session)
                  }}</span>
                </span>
              </button>
              <UiTooltip content="Archive session">
                <UiIconButton
                  v-if="canArchiveSession(project, session)"
                  class="session-archive"
                  size="md"
                  variant="reveal"
                  :label="`Archive ${session.title}`"
                  @click="() => archiveSession(project, session)"
                >
                  <UiIcon name="archive" />
                </UiIconButton>
              </UiTooltip>
            </div>
          </UiContextMenu>
          <div
            v-if="projectSessions(project).length === 0"
            class="empty-sessions"
          >
            No active sessions
          </div>
        </div>
      </div>

      <div
        v-if="state.workspace && state.workspace.projects.length === 0"
        class="empty-projects"
      >
        <UiIcon name="folder" />
        <span>No projects yet</span>
      </div>
    </div>

    <footer class="sidebar-footer">
      <div class="project-menu-wrap">
        <UiMenu
          v-model:open="projectMenuOpen"
          :items="projectMenuItems"
          :min-width="176"
        >
          <template #trigger>
            <UiIconButton
              size="lg"
              label="Open project"
              title="Open project"
            >
              <UiIcon name="folder" />
            </UiIconButton>
          </template>
        </UiMenu>
      </div>
    </footer>
  </aside>
</template>

<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window';
import Sortable, { type SortableEvent } from 'sortablejs';
import { onBeforeUnmount, onMounted, ref } from 'vue';

import type { ProjectSummary, SessionSummary } from '../composables/state';
import useTau from '../composables/useTau';

import UiContextMenu from './ui/UiContextMenu.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiMenu from './ui/UiMenu.vue';
import type { UiMenuItem } from './ui/UiMenu.vue';
import UiStatusDot from './ui/UiStatusDot.vue';
import UiTooltip from './ui/UiTooltip.vue';

const {
  addLocalProject,
  archiveSession,
  canArchiveSession,
  indicatorLabel,
  isSessionSelected,
  isSessionUnread,
  markSessionRead,
  markSessionUnread,
  newSession,
  openRemoteProjectDialog,
  projectIndicator,
  projectSessions,
  removeProject,
  reorderProjects,
  selectSession,
  sessionIndicator,
  sessionLastActive,
  state,
  toggleProject,
} = useTau();

const projectList = ref<HTMLElement>();
const projectMenuOpen = ref(false);
let projectSortable: Sortable | undefined;

onMounted(() => {
  setupProjectReordering();
});
onBeforeUnmount(() => {
  projectSortable?.destroy();
});

/** The project menu's two ways to add a project. */
const projectMenuItems: UiMenuItem[] = [
  {
    label: 'Open Local Project',
    run: () => void addLocalProject(),
  },
  {
    label: 'Open Remote Project',
    run: () => void openRemoteProjectDialog(),
  },
];

function sessionMenuItems(
  project: ProjectSummary,
  session: SessionSummary,
): UiMenuItem[] {
  const unread = isSessionUnread(project, session);
  const items: UiMenuItem[] = [
    {
      label: unread ? 'Mark as Read' : 'Mark as Unread',
      run: () =>
        unread
          ? markSessionRead(project, session)
          : markSessionUnread(project, session),
    },
  ];
  if (canArchiveSession(project, session)) {
    items.push({
      label: 'Archive Session',
      run: () => void archiveSession(project, session),
    });
  }
  return items;
}

function setupProjectReordering(): void {
  if (!projectList.value) return;
  projectSortable = Sortable.create(projectList.value, {
    animation: 180,
    handle: '.project-drag-handle',
    draggable: '.project-group',
    ghostClass: 'project-sortable-ghost',
    chosenClass: 'project-sortable-chosen',
    dragClass: 'project-sortable-drag',
    forceFallback: true,
    fallbackOnBody: true,
    fallbackTolerance: 3,
    onEnd: finishProjectReordering,
  });
}

function finishProjectReordering(event: SortableEvent): void {
  if (event.oldIndex === undefined || event.newIndex === undefined) return;
  void reorderProjects(event.oldIndex, event.newIndex);
}

function handleTitlebarMouseDown(event: MouseEvent): void {
  if (
    event.button !== 0 ||
    event.detail > 1 ||
    isTitlebarControl(event.target)
  ) {
    return;
  }
  void getCurrentWindow()
    .startDragging()
    .catch(() => undefined);
}

function handleTitlebarDoubleClick(event: MouseEvent): void {
  if (event.button !== 0 || isTitlebarControl(event.target)) return;
  event.preventDefault();
  void getCurrentWindow()
    .toggleMaximize()
    .catch(() => undefined);
}

function isTitlebarControl(target: EventTarget | null): boolean {
  return (
    !(target instanceof Element) ||
    Boolean(target.closest('button, a, input, select, textarea'))
  );
}
</script>

<style scoped>
.sidebar {
  display: flex;
  position: relative;
  flex-direction: column;
  min-width: 0;
  border-right: 1px solid var(--border);
  background: var(--panel);
}
</style>

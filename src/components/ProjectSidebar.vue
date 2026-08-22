<template>
  <aside
    ref="sidebar"
    class="sidebar"
    aria-label="Projects and sessions"
  >
    <header
      class="sidebar-titlebar"
      @mousedown="handleTitlebarMouseDown"
      @dblclick="handleTitlebarDoubleClick"
    ></header>

    <div
      ref="projectList"
      class="project-list"
      @pointerenter="holdSessionOrder"
      @pointerleave="releaseSessionOrder"
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
            <UiStatusDot
              v-if="projectIndicator(project)"
              class="project-status"
              :tone="projectIndicator(project) || undefined"
              :label="indicatorLabel(projectIndicator(project))"
            />
            <span>{{ project.name }}</span>
            <UiIcon
              name="chevron"
              :class="{ expanded: !project.collapsed }"
            />
          </button>
          <UiIconButton
            class="row-action"
            size="md"
            variant="reveal"
            :label="`New session in ${project.name}`"
            title="New session"
            @click="() => newSession(project)"
          >
            <UiIcon name="plus" />
          </UiIconButton>
          <UiIconButton
            class="row-action"
            size="md"
            variant="reveal"
            tone="danger"
            :label="`Remove ${project.name}`"
            title="Remove project"
            @click="() => removeProject(project)"
          >
            <UiIcon name="trash" />
          </UiIconButton>
        </div>

        <div
          v-if="!project.collapsed"
          class="session-list"
        >
          <UiContextMenu
            v-for="session in orderedSessions(project)"
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
                :aria-current="
                  isSessionSelected(project, session) ? 'page' : undefined
                "
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
              <UiIconButton
                v-if="canArchiveSession(project, session)"
                class="session-archive"
                size="md"
                variant="reveal"
                :label="`Archive ${session.title}`"
                title="Archive session"
                @click="() => archiveSession(project, session)"
              >
                <UiIcon name="archive" />
              </UiIconButton>
            </div>
          </UiContextMenu>
          <div
            v-if="orderedSessions(project).length === 0"
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
      <IssueReportPopover
        :session-id="state.activeSessionId || undefined"
        :submit-report="submitIssueReport"
      />
    </footer>

    <div
      class="sidebar-resize-handle"
      role="separator"
      aria-label="Resize sidebar"
      aria-orientation="vertical"
      :aria-valuemin="MIN_SIDEBAR_WIDTH"
      :aria-valuemax="MAX_SIDEBAR_WIDTH"
      :aria-valuenow="sidebarWidth"
      tabindex="0"
      @pointerdown="startSidebarResize"
      @pointermove="handleSidebarResize"
      @pointerup="finishSidebarResize"
      @pointercancel="stopSidebarResize"
      @lostpointercapture="stopSidebarResize"
      @keydown="handleSidebarResizeKeydown"
    ></div>
  </aside>
</template>

<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window';
import Sortable, { type SortableEvent } from 'sortablejs';
import { onBeforeUnmount, onMounted, ref } from 'vue';

import type { ProjectSummary, SessionSummary } from '../composables/state';
import useTau from '../composables/useTau';
import { applyHeldOrder, heldSessionIds } from '../lib/session-order';
import {
  MAX_SIDEBAR_WIDTH,
  MIN_SIDEBAR_WIDTH,
  clampSidebarWidth,
  persistSidebarWidth,
} from '../lib/sidebar-width';

import IssueReportPopover from './IssueReportPopover.vue';
import UiContextMenu from './ui/UiContextMenu.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiMenu from './ui/UiMenu.vue';
import type { UiMenuItem } from './ui/UiMenu.vue';
import UiStatusDot from './ui/UiStatusDot.vue';

const props = defineProps<{
  sidebarWidth: number;
  resizing: boolean;
}>();

const emit = defineEmits<{
  'update:sidebar-width': [value: number];
  'update:resizing': [value: boolean];
}>();

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
  submitIssueReport,
  toggleProject,
} = useTau();

const projectList = ref<HTMLElement>();
const sidebar = ref<HTMLElement>();
const projectMenuOpen = ref(false);
/** Held session ids by project path, empty whenever the list is not hovered. */
const heldOrder = ref(new Map<string, string[]>());
let projectSortable: Sortable | undefined;

onMounted(() => {
  setupProjectReordering();
});
onBeforeUnmount(() => {
  projectSortable?.destroy();
});

/**
 * Freezes the session order under the pointer. Only a mouse aims at a row it
 * can see; a touch or pen lands where it lands, and holding for one would
 * leave the order frozen until the next time a pointer happens to leave.
 */
function holdSessionOrder(event: PointerEvent): void {
  if (event.pointerType !== 'mouse') return;
  heldOrder.value = new Map(
    (state.workspace?.projects ?? []).map((project) => [
      project.path,
      heldSessionIds(projectSessions(project)),
    ]),
  );
}

function releaseSessionOrder(): void {
  if (heldOrder.value.size > 0) heldOrder.value = new Map();
}

function orderedSessions(project: ProjectSummary): SessionSummary[] {
  return applyHeldOrder(
    projectSessions(project),
    heldOrder.value.get(project.path) ?? [],
  );
}

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

function startSidebarResize(event: PointerEvent): void {
  if (event.button !== 0) return;
  const handle = event.currentTarget;
  if (!(handle instanceof HTMLElement)) return;

  event.preventDefault();
  emit('update:resizing', true);
  handle.setPointerCapture(event.pointerId);
  handle.focus({ preventScroll: true });
  updateSidebarWidth(event.clientX);
}

function handleSidebarResize(event: PointerEvent): void {
  if (props.resizing) updateSidebarWidth(event.clientX);
}

function finishSidebarResize(event: PointerEvent): void {
  if (!props.resizing) return;
  updateSidebarWidth(event.clientX);
  stopSidebarResize();
}

function stopSidebarResize(): void {
  if (!props.resizing) return;
  emit('update:resizing', false);
  persistSidebarWidth(props.sidebarWidth);
}

function handleSidebarResizeKeydown(event: KeyboardEvent): void {
  const step = event.shiftKey ? 40 : 10;
  let nextWidth: number;

  switch (event.key) {
    case 'ArrowLeft':
      nextWidth = props.sidebarWidth - step;
      break;
    case 'ArrowRight':
      nextWidth = props.sidebarWidth + step;
      break;
    case 'Home':
      nextWidth = MIN_SIDEBAR_WIDTH;
      break;
    case 'End':
      nextWidth = MAX_SIDEBAR_WIDTH;
      break;
    default:
      return;
  }

  event.preventDefault();
  emit('update:sidebar-width', clampSidebarWidth(nextWidth));
  persistSidebarWidth(clampSidebarWidth(nextWidth));
}

function updateSidebarWidth(pointerX: number): void {
  const left = sidebar.value?.getBoundingClientRect().left ?? 0;
  emit('update:sidebar-width', clampSidebarWidth(pointerX - left));
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

.sidebar-resize-handle {
  position: absolute;
  z-index: 4;
  top: 0;
  right: -4px;
  bottom: 0;
  width: 9px;
  cursor: col-resize;
  touch-action: none;
}

.sidebar-resize-handle::after {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  left: 4px;
  width: 1px;
  background: transparent;
}

.sidebar-resize-handle:hover::after,
.sidebar-resize-handle:focus-visible::after {
  background: var(--muted);
}

.sidebar-resize-handle:focus-visible {
  outline: 0;
}

.sidebar-titlebar {
  display: flex;
  flex: none;
  align-items: center;
  height: 30px;
  min-height: 30px;
  border-bottom: 1px solid var(--border);
}

.sidebar-footer {
  display: flex;
  flex: none;
  justify-content: space-between;
  padding: 6px;
}

.project-list {
  flex: 1;
  padding: 6px;
  overflow: auto;
}

.project-row {
  display: flex;
  position: relative;
  align-items: center;
  min-width: 0;
  border-radius: 7px;
  gap: 0;
}

.project-row:hover {
  background: var(--hover);
}

.project-drag-handle {
  display: grid;
  position: absolute;
  z-index: 2;
  top: 50%;
  left: 3px;
  width: 25px;
  height: 29px;

  /* Centered on the row, then nudged onto the label's optical center. */
  transform: translateY(calc(-50% + 1px));
  color: var(--muted);
  touch-action: none;
  place-items: center;
}

.project-drag-handle svg {
  width: 9px;
  height: 13px;
  transform: translateX(-8px);
  transition:
    opacity 110ms ease,
    transform 160ms cubic-bezier(0.2, 0.8, 0.2, 1);
  opacity: 0;
}

.project-row:hover > .project-drag-handle svg,
.project-sortable-chosen > .project-row .project-drag-handle svg,
.project-sortable-drag > .project-row .project-drag-handle svg {
  transform: translateX(0);
  opacity: 1;
}

.project-toggle {
  display: flex;
  flex: 1;
  align-items: center;
  min-width: 0;
  padding: 7px 5px;
  transition: padding-left 160ms cubic-bezier(0.2, 0.8, 0.2, 1);
  background: transparent;
  color: var(--muted);
  text-align: left;
  gap: 3px;
}

/* stylelint-disable-next-line no-descending-specificity */
.project-toggle svg {
  flex: none;
  width: 13px;
  transform: translateY(1px);
  transition: transform 120ms ease;
  color: var(--muted);
}

.project-toggle svg.expanded {
  transform: translateY(1px) rotate(90deg);
}

/* Nudged down with the chevron so both sit on the label's optical center. */
.project-status {
  margin-right: 4px;
  transform: translateY(1px);
}

.project-toggle span {
  overflow: hidden;
  font-size: 13px;
  font-weight: 400;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-row.selected .project-toggle span {
  color: var(--text);
}

.project-row:hover > .project-toggle,
.project-sortable-chosen > .project-row .project-toggle,
.project-sortable-drag > .project-row .project-toggle {
  padding-left: 27px;
}

.project-sortable-ghost {
  opacity: 0.2;
}

.project-sortable-chosen > .project-row,
.project-sortable-drag > .project-row {
  background: var(--hover);
}

.project-sortable-drag {
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: 8px;
  opacity: 0.98;
  background: var(--panel-raised);
  box-shadow: 0 15px 30px var(--shadow-strong);
}

/* A row-hovered action steps out of its row; its box and states live in UiIconButton. */
.project-row:hover :deep(.row-action),
.project-row:focus-within :deep(.row-action) {
  opacity: 0.65;
}

.session-list {
  margin: 1px 0 5px;
}

.session-row,
.empty-sessions {
  height: 29px;
}

.session-row {
  display: flex;
  position: relative;
  width: 100%;
  min-width: 0;
  border-radius: 7px;
  background: transparent;
}

.session-row:hover {
  background: var(--hover);
}

.session-row.selected {
  background: var(--selected);
}

.session-select {
  display: flex;
  align-items: center;
  width: 100%;
  min-width: 0;
  padding: 7px;
  background: transparent;
  text-align: left;
  gap: 7px;
}

/* The archive action floats over its row; its box and states live in UiIconButton. */
.session-archive {
  position: absolute;
  top: 50%;
  right: 0;
  transform: translateY(-50%);
}

.session-row:hover :deep(.session-archive),
.session-row:focus-within :deep(.session-archive) {
  opacity: 0.65;
}

.session-row.archivable:hover .session-time,
.session-row.archivable:focus-within .session-time {
  opacity: 0;
}

.session-copy {
  display: flex;
  flex: 1;
  align-items: baseline;
  min-width: 0;
  gap: 7px;
}

.session-title {
  flex: 1;
  overflow: hidden;
  color: var(--text);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* stylelint-disable-next-line no-descending-specificity */
.session-time {
  flex: none;
  color: var(--faint);
  font-size: 10px;
}

.empty-sessions {
  display: flex;
  align-items: center;
  padding: 0 5px;
  color: var(--faint);
  font-size: 11px;
}

.empty-projects {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 180px;
  color: var(--muted);
  font-size: 12px;
  gap: 8px;
}

/* stylelint-disable-next-line no-descending-specificity */
.empty-projects > svg {
  width: 24px;
  height: 24px;
  color: var(--faint);
}
</style>

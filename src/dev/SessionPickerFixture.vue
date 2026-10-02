<template>
  <main class="session-picker-fixture">
    <button
      type="button"
      @click="openPicker"
    >
      Open Switch Session
    </button>
    <output data-testid="selected-session">{{ selectedDestination }}</output>
    <CommandPalette
      v-model:open="open"
      v-model:query="query"
      v-model:selected-id="selectedId"
      :rows="rows"
      page-title="Switch Session"
      placeholder="Search sessions…"
      nested
      @back="closePicker"
      @select="select"
    />
  </main>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue';

import CommandPalette, {
  type CommandPaletteRow,
} from '../components/CommandPalette.vue';
import type { ProjectSummary, SessionSummary } from '../composables/state';
import {
  preferredSessionPickerId,
  resolveSessionPickerTarget,
  sessionPickerEntries,
} from '../lib/session-picker';

interface SessionPickerFixtureApi {
  armStaleRemoval(projectPath: string, sessionId: string): void;
  collapseProject(projectPath: string): void;
  collapseAll(): void;
}

declare global {
  interface Window {
    __TAU_SESSION_PICKER_FIXTURE__?: SessionPickerFixtureApi;
  }
}

function session(id: string, title: string): SessionSummary {
  return {
    id,
    path: `/fixture/${id}.jsonl`,
    title,
    lastActive: 'just now',
    lastUserMessageAt: 0,
    sortAt: 0,
    archived: false,
    selected: false,
  };
}

function project(
  path: string,
  name: string,
  sessions: SessionSummary[],
  connectionString?: string,
): ProjectSummary {
  return {
    path,
    name,
    workingDirectory: path,
    connectionString,
    collapsed: false,
    selected: false,
    sessions,
  };
}

const projects = ref<ProjectSummary[]>([
  project(
    '/fixture/earlier-a',
    'Earlier A',
    Array.from({ length: 10 }, (_, index) =>
      session(`earlier-a-${index}`, `Earlier A ${index}`),
    ),
  ),
  project(
    '/fixture/earlier-b',
    'Earlier B',
    Array.from({ length: 10 }, (_, index) =>
      session(`earlier-b-${index}`, `Earlier B ${index}`),
    ),
  ),
  project('/fixture/duplicate-a', 'Duplicate project', [
    session('shared-session', 'Duplicate session'),
  ]),
  project('/fixture/duplicate-b', 'Duplicate project', [
    session('shared-session', 'Duplicate session'),
  ]),
  project(
    '/fixture/remote',
    'Remote project',
    [session('remote-session', 'Remote destination')],
    'ssh://fixture@example.test',
  ),
]);
const activeProjectPath = ref('/fixture/duplicate-b');
const activeSessionId = ref('shared-session');
const open = ref(true);
const query = ref('');
const selectedId = ref(
  preferredSessionPickerId(
    sessionPickerEntries(projects.value, (item) => item.sessions),
    activeProjectPath.value,
    activeSessionId.value,
  ),
);
const selectedDestination = ref(
  `${activeProjectPath.value}/${activeSessionId.value}`,
);
let staleRemoval: { projectPath: string; sessionId: string } | undefined;

const rows = computed<CommandPaletteRow[]>(() =>
  sessionPickerEntries(projects.value, (item) => item.sessions).map(
    (entry) => ({
      id: entry.id,
      title: entry.session.title,
      detail:
        entry.projectPath === activeProjectPath.value &&
        entry.sessionId === activeSessionId.value
          ? 'Current'
          : entry.session.lastActive,
      section: { id: entry.projectPath, label: entry.projectName },
    }),
  ),
);

function openPicker(): void {
  open.value = true;
}

function closePicker(): void {
  open.value = false;
}

function applyStaleRemoval(): void {
  if (!staleRemoval) return;
  const { projectPath, sessionId } = staleRemoval;
  staleRemoval = undefined;
  const project = projects.value.find((item) => item.path === projectPath);
  if (project)
    project.sessions = project.sessions.filter((item) => item.id !== sessionId);
}

function select(id: string): void {
  applyStaleRemoval();
  const target = resolveSessionPickerTarget(
    id,
    projects.value,
    (project) => project.sessions,
  );
  if (!target) return;
  activeProjectPath.value = target.project.path;
  activeSessionId.value = target.session.id;
  selectedDestination.value = `${target.project.path}/${target.session.id}`;
  open.value = false;
}

window.__TAU_SESSION_PICKER_FIXTURE__ = {
  armStaleRemoval(projectPath, sessionId): void {
    staleRemoval = { projectPath, sessionId };
  },
  collapseProject(projectPath): void {
    const project = projects.value.find((item) => item.path === projectPath);
    if (project) project.collapsed = true;
  },
  collapseAll(): void {
    for (const project of projects.value) project.collapsed = true;
  },
};

onBeforeUnmount(() => {
  delete window.__TAU_SESSION_PICKER_FIXTURE__;
});
</script>

<style scoped>
.session-picker-fixture {
  padding: 24px;
}
</style>

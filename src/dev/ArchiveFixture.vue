<template>
  <main
    class="archive-fixture"
    :style="{ '--fixture-sidebar-width': `${sidebarWidth}px` }"
  >
    <ProjectSidebar
      v-model:sidebar-width="sidebarWidth"
      v-model:resizing="resizing"
    />
    <div data-testid="outside-sidebar">Outside sidebar</div>
  </main>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue';

import ProjectSidebar from '../components/ProjectSidebar.vue';
import {
  state,
  type ProjectSummary,
  type SessionSummary,
  type WorkspaceSnapshot,
} from '../composables/state';

interface ArchiveFixtureApi {
  replace(count: number): void;
  remove(projectPath: string, id: string): void;
}
declare global {
  interface Window {
    __TAU_ARCHIVE_FIXTURE__?: ArchiveFixtureApi;
  }
}

const sidebarWidth = ref(300);
const resizing = ref(false);
const today = new Date();
const day = 86_400_000;

function createWorkspace(count: number): WorkspaceSnapshot {
  const projects: ProjectSummary[] = Array.from({ length: 3 }, (_, index) => ({
    path: `/fixture/project-${index}`,
    name: `Project ${index}`,
    workingDirectory: `/fixture/project-${index}`,
    collapsed: false,
    selected: false,
    sessions: [],
  }));
  for (let index = 0; index < count; index++) {
    const project = projects[index % projects.length]!;
    const sortAt =
      today.getTime() - Math.floor(index / 120) * day - (index % 120);
    const session: SessionSummary = {
      id: `archive-${index}`,
      path: `${project.path}/archive-${index}.jsonl`,
      title: `Archive ${index}`,
      model: 'fixture',
      lastActive: 'recently',
      lastUserMessageAt: sortAt,
      sortAt,
      archived: true,
      selected: false,
    };
    project.sessions.push(session);
  }
  return {
    activeProjectPath: projects[0]!.path,
    piPath: '/fixture/bin/pi',
    projects,
  };
}

state.workspace = createWorkspace(2500);
state.activeProjectPath = state.workspace.activeProjectPath;
state.activeSessionId = '';
state.activeSessionPath = '';
state.activeControllerKey = '';
state.controllers = [];
state.ephemeralSessions = [];
window.__TAU_ARCHIVE_FIXTURE__ = {
  replace(count): void {
    state.workspace = createWorkspace(count);
  },
  remove(path, id): void {
    const project = state.workspace?.projects.find(
      (item) => item.path === path,
    );
    if (project)
      project.sessions = project.sessions.filter((item) => item.id !== id);
  },
};
onBeforeUnmount(() => {
  delete window.__TAU_ARCHIVE_FIXTURE__;
});
</script>

<style scoped>
.archive-fixture {
  display: grid;
  grid-template-columns: var(--fixture-sidebar-width) 1fr;
  grid-template-rows: minmax(0, 1fr);
  width: 100vw;
  height: 100vh;
}

.archive-fixture :deep(.sidebar) {
  width: var(--fixture-sidebar-width);
  min-height: 0;
}

[data-testid='outside-sidebar'] {
  padding: 40px;
}
</style>

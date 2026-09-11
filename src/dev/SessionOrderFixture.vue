<template>
  <main
    class="session-order-fixture"
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
import { state } from '../composables/state';
import type {
  ProjectSummary,
  SessionSummary,
  WorkspaceSnapshot,
} from '../composables/state';

interface SessionOrderFixtureApi {
  addSession(projectPath: string, id: string, sortAt: number): void;
  materializeSession(
    projectPath: string,
    sessionId: string,
    materializedId: string,
  ): void;
  updateActivity(projectPath: string, sessionId: string, sortAt: number): void;
}

declare global {
  interface Window {
    __TAU_SESSION_ORDER_FIXTURE__?: SessionOrderFixtureApi;
  }
}

const sidebarWidth = ref(300);
const resizing = ref(false);

function session(id: string, sortAt: number): SessionSummary {
  return {
    id,
    path: `/fixture/${id}.jsonl`,
    title: id,
    lastActive: 'now',
    lastUserMessageAt: 0,
    sortAt,
    archived: false,
    selected: false,
  };
}

function project(
  path: string,
  name: string,
  sessions: SessionSummary[],
): ProjectSummary {
  return {
    path,
    name,
    workingDirectory: path,
    collapsed: false,
    selected: false,
    sessions,
  };
}

const workspace: WorkspaceSnapshot = {
  activeProjectPath: '/fixture/alpha',
  piPath: '/fixture/bin/pi',
  projects: [
    project('/fixture/alpha', 'Alpha project', [
      session('Alpha recent', 30),
      session('Alpha older', 20),
    ]),
    project('/fixture/empty', 'Empty project', []),
    project('/fixture/beta', 'Beta project', [
      session('Beta recent', 15),
      session('Beta older', 10),
    ]),
  ],
};
state.workspace = workspace;
state.activeProjectPath = workspace.activeProjectPath;
state.activeSessionId = '';
state.activeSessionPath = '';
state.activeControllerKey = '';
state.controllers = [];
state.ephemeralSessions = [];

function fixtureProject(path: string): ProjectSummary {
  const match = state.workspace?.projects.find((item) => item.path === path);
  if (!match) throw new Error(`Unknown fixture project ${path}`);
  return match;
}

window.__TAU_SESSION_ORDER_FIXTURE__ = {
  addSession(projectPath, id, sortAt): void {
    fixtureProject(projectPath).sessions.push(session(id, sortAt));
  },
  materializeSession(projectPath, sessionId, materializedId): void {
    const sessions = fixtureProject(projectPath).sessions;
    const index = sessions.findIndex((item) => item.id === sessionId);
    const match = sessions[index];
    if (!match) throw new Error(`Unknown fixture session ${sessionId}`);
    match.id = materializedId;
    // Registration replaces the mutable ephemeral object with the native
    // workspace row that now owns the same materialized identity.
    sessions.splice(index, 1, { ...match });
  },
  updateActivity(projectPath, sessionId, sortAt): void {
    const match = fixtureProject(projectPath).sessions.find(
      (item) => item.id === sessionId,
    );
    if (!match) throw new Error(`Unknown fixture session ${sessionId}`);
    match.sortAt = sortAt;
  },
};

onBeforeUnmount(() => {
  delete window.__TAU_SESSION_ORDER_FIXTURE__;
});
</script>

<style scoped>
.session-order-fixture {
  display: grid;
  grid-template-columns: var(--fixture-sidebar-width) 1fr;
  width: 100vw;
  height: 100vh;
}

.session-order-fixture :deep(.sidebar) {
  width: var(--fixture-sidebar-width);
}

[data-testid='outside-sidebar'] {
  padding: 40px;
}
</style>

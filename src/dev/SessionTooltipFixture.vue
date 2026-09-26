<template>
  <main
    class="session-tooltip-fixture"
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
  SessionController,
  WorkspaceSnapshot,
} from '../composables/state';

type TooltipStatus = 'working' | 'draft' | 'unread' | 'idle';

interface SessionTooltipFixtureApi {
  setTitle(title: string): void;
  setMarkdown(source: string): void;
  setStatus(status: TooltipStatus): void;
}

declare global {
  interface Window {
    __TAU_SESSION_TOOLTIP_FIXTURE__?: SessionTooltipFixtureApi;
    __TAU_TOOLTIP_OPENED_URLS__?: string[];
  }
}

const sidebarWidth = ref(280);
const resizing = ref(false);

const longTitle =
  '<strong>Do not render markup</strong> — this intentionally long session title is clipped in the sidebar';

function session(
  id: string,
  title: string,
  lastActive: string,
  archived = false,
): SessionSummary {
  return {
    id,
    path: `/fixture/session-tooltip/${id}.jsonl`,
    title,
    lastActive,
    lastUserMessageAt: 0,
    sortAt: 0,
    archived,
    selected: false,
  };
}

const workspace: WorkspaceSnapshot = {
  activeProjectPath: '/fixture/session-tooltip',
  piPath: '/fixture/bin/pi',
  projects: [
    {
      path: '/fixture/session-tooltip',
      name: 'Tooltip fixture',
      workingDirectory: '/fixture/session-tooltip',
      collapsed: false,
      selected: false,
      sessions: [
        {
          ...session('working-session-opaque-7fb4d9', longTitle, '2h'),
          titleMarkdown:
            '**Markdown preview** with `inline code`\n\n- First item\n  - Nested item\n\n```js\nconst ready = true;\n```',
        },
        session('draft-session-opaque-a12c8e', 'Prepare release notes', '1d'),
        session(
          'unread-session-opaque-3e5f10',
          'Check the browser regression results',
          '1w',
        ),
        {
          ...session(
            'archived-session-opaque-c0ffee',
            '<em>Archived markup stays text</em> — a long archived session title',
            '3w',
            true,
          ),
          titleMarkdown: 'Archived *notes*\n\nSecond paragraph with `code`',
        },
      ],
    } satisfies ProjectSummary,
  ],
};

state.workspace = workspace;
state.activeProjectPath = workspace.activeProjectPath;
state.activeSessionId = '';
state.activeSessionPath = '';
state.activeControllerKey = '';
state.controllers = [
  {
    key: 'tooltip-working-controller',
    projectPath: '/fixture/session-tooltip',
    sessionId: 'working-session-opaque-7fb4d9',
    working: true,
    draft: '',
    unread: false,
  } as SessionController,
];
state.ephemeralSessions = [];

function primarySession(): SessionSummary {
  const session = state.workspace?.projects[0]?.sessions[0];
  if (!session)
    throw new Error('Expected the primary tooltip fixture session.');
  return session;
}

window.__TAU_SESSION_TOOLTIP_FIXTURE__ = {
  setTitle(title): void {
    primarySession().title = title;
    primarySession().titleMarkdown = title;
  },
  setMarkdown(source): void {
    primarySession().titleMarkdown = source;
  },
  setStatus(status): void {
    const controller = state.controllers[0];
    if (!controller)
      throw new Error('Expected the tooltip fixture controller.');
    controller.working = status === 'working';
    controller.draft = status === 'draft' ? 'Unsent draft' : '';
    controller.unread = status === 'unread';
  },
};

onBeforeUnmount(() => {
  delete window.__TAU_SESSION_TOOLTIP_FIXTURE__;
});
</script>

<style scoped>
.session-tooltip-fixture {
  display: grid;
  grid-template-columns: var(--fixture-sidebar-width) 1fr;
  width: 100vw;
  height: 100vh;
}

.session-tooltip-fixture :deep(.sidebar) {
  width: var(--fixture-sidebar-width);
}

[data-testid='outside-sidebar'] {
  padding: 40px;
}
</style>

<template>
  <PopoverRoot v-model:open="open">
    <PopoverTrigger as-child>
      <button
        class="update-trigger"
        :class="{ 'first-run-trigger': firstRun }"
        type="button"
        :aria-label="triggerLabel"
      >
        <span
          v-if="indicator === 'accent' || indicator === 'downloading'"
          class="update-indicator"
          :class="indicator"
          aria-hidden="true"
        ></span>
        <UiIcon
          v-else-if="indicator === 'failure'"
          class="update-failure-mark"
          name="cross"
        />
        <template v-if="firstRun">
          <span class="first-run-name">tau</span>
          <span class="version-number">{{ appVersion }}</span>
        </template>
        <template v-else>v{{ appVersion }}</template>
      </button>
    </PopoverTrigger>
    <PopoverPortal>
      <PopoverContent
        class="update-popover ui-surface"
        side="top"
        align="end"
        :side-offset="5"
        aria-label="Tau Update"
        :aria-busy="update.inProgress.value || undefined"
      >
        <div
          class="update-copy"
          role="status"
          aria-live="polite"
        >
          <strong>{{ title }}</strong>
          <span>{{ description }}</span>
        </div>

        <progress
          v-if="showProgress && progressPercent !== undefined"
          :value="progressPercent"
          max="100"
          :aria-label="`Download ${progressPercent}% complete`"
        >
          {{ progressPercent }}%
        </progress>
        <div
          v-else-if="showProgress"
          class="indeterminate-progress"
          role="progressbar"
          aria-label="Downloading update"
        ></div>

        <footer class="update-actions">
          <UiButton
            v-if="state.phase === 'available'"
            size="md"
            :disabled="update.inProgress.value"
            @click="dismiss"
          >
            Not Now
          </UiButton>
          <UiButton
            v-if="primaryAction"
            variant="primary"
            size="md"
            :disabled="update.inProgress.value"
            @click="runPrimaryAction"
          >
            {{ primaryAction }}
          </UiButton>
        </footer>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>

<script setup lang="ts">
import {
  PopoverContent,
  PopoverPortal,
  PopoverRoot,
  PopoverTrigger,
} from 'reka-ui';
import { computed, ref, watch } from 'vue';

import appVersion from '../lib/app-version';
import { updateFailureDescription, useUpdate } from '../lib/update';

import UiButton from './ui/UiButton.vue';
import UiIcon from './ui/UiIcon.vue';

withDefaults(defineProps<{ firstRun?: boolean }>(), { firstRun: false });

const update = useUpdate();
const state = update.state;
const open = ref(false);

const indicator = computed<'accent' | 'downloading' | 'failure' | undefined>(
  () => {
    if (state.failureUnread) return 'failure';
    if (state.phase === 'downloading' || state.phase === 'verifying')
      return 'downloading';
    if (
      state.phase === 'available' ||
      state.phase === 'awaiting-confirmation' ||
      state.phase === 'restart-needed'
    )
      return 'accent';
    return undefined;
  },
);
const triggerLabel = computed(() => {
  const suffix = indicator.value === 'failure' ? ', update failed' : '';
  return `Tau version ${appVersion}${suffix}`;
});
const showProgress = computed(
  () => state.phase === 'downloading' || state.phase === 'verifying',
);
const progressPercent = computed(() => {
  if (!state.totalBytes) return undefined;
  return Math.min(
    100,
    Math.round((state.downloadedBytes / state.totalBytes) * 100),
  );
});
const title = computed(() => {
  switch (state.phase) {
    case 'checking':
      return 'Checking for updates';
    case 'available':
      return `Version ${state.version ?? ''} available`;
    case 'downloading':
      return 'Downloading update';
    case 'verifying':
      return 'Verifying update';
    case 'awaiting-confirmation':
      return 'Update ready';
    case 'installing':
      return 'Installing update';
    case 'restart-needed':
      return 'Restart needed';
    case 'failure':
      return 'Update failed';
    case 'current':
      return 'Tau is up to date';
    default:
      return 'Updates unavailable';
  }
});
const description = computed(() => {
  switch (state.phase) {
    case 'checking':
      return 'This should only take a moment.';
    case 'available':
      return 'Download and restart when you are ready.';
    case 'downloading':
      return progressPercent.value === undefined
        ? 'Downloading…'
        : `${progressPercent.value}%`;
    case 'verifying':
      return 'Preparing to install.';
    case 'awaiting-confirmation':
      return 'Restart Tau to install the update.';
    case 'installing':
      return 'Tau will restart when installation finishes.';
    case 'restart-needed':
      return 'Quit and reopen Tau to finish updating.';
    case 'failure':
      return updateFailureDescription(state.failureCategory);
    case 'current':
      return `Version ${appVersion} is installed.`;
    default:
      return 'Updates are not supported in this build.';
  }
});
const primaryAction = computed(() => {
  switch (state.phase) {
    case 'available':
    case 'failure':
      return 'Update';
    case 'awaiting-confirmation':
      return 'Update and Restart';
    case 'current':
    case 'unavailable':
      return 'Check for Updates';
    default:
      return undefined;
  }
});

watch(
  () => state.revealToken,
  () => {
    if (update.consumeReveal()) open.value = true;
  },
  { immediate: true },
);

watch(open, (isOpen) => {
  if (isOpen) update.acknowledgeFailure();
});

function dismiss(): void {
  void update.dismiss();
  open.value = false;
}

function runPrimaryAction(): void {
  if (state.phase === 'available' || state.phase === 'failure') {
    void update.download();
  } else if (state.phase === 'awaiting-confirmation') {
    void update.requestInstall();
  } else {
    void update.check(true);
  }
}
</script>

<style scoped>
.update-trigger {
  display: flex;
  position: relative;
  top: 1px;
  align-items: center;
  padding: 5px 6px;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: var(--text-xs);
  line-height: var(--leading-tight);
  white-space: nowrap;
  gap: 4px;
}

.update-trigger:hover,
.update-trigger:focus-visible,
.update-trigger[data-state='open'] {
  outline: 0;
  background: var(--hover);
  color: var(--text);
}

.update-indicator {
  width: 5px;
  height: 5px;
  border-radius: 50%;
}

.update-indicator.accent {
  background: var(--accent);
}

.update-indicator.downloading {
  background: var(--muted);
}

.update-failure-mark {
  width: 8px;
  height: 8px;
  color: var(--danger);
}

.first-run-trigger {
  top: 0;
  margin: 0 0 14px;
  padding: 4px 6px;
  gap: 8px;
}

.first-run-name {
  color: var(--text);
}

.version-number {
  color: var(--faint);
}

:global(.update-popover) {
  display: flex;
  z-index: 20;
  flex-direction: column;
  width: min(250px, calc(100vw - 18px));
  padding: 11px;
  outline: 0;
  gap: 8px;
}

.update-copy {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.update-copy strong {
  color: var(--text);
  font-size: var(--text-sm);
  font-weight: 600;
}

.update-copy span {
  color: var(--muted);
  font-size: var(--text-xs);
  line-height: var(--leading-ui);
}

progress,
.indeterminate-progress {
  width: 100%;
  height: 3px;
  overflow: hidden;
  border: 0;
  border-radius: 2px;
  background: var(--border);
}

progress {
  appearance: none;
}

progress::-webkit-progress-bar {
  background: var(--border);
}

progress::-webkit-progress-value {
  background: var(--muted);
}

.indeterminate-progress::after {
  content: '';
  display: block;
  width: 35%;
  height: 100%;
  background: var(--muted);
}

.update-actions {
  display: flex;
  justify-content: flex-end;
  gap: 5px;
}

.update-actions:empty {
  display: none;
}
</style>

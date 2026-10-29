<template>
  <UiTooltip text="Report an Issue">
    <span class="issue-report-trigger">
      <PopoverRoot
        :open="open"
        @update:open="setOpen"
      >
        <PopoverTrigger as-child>
          <UiIconButton
            size="lg"
            label="Report an Issue"
            ><UiIcon name="bug"
          /></UiIconButton>
        </PopoverTrigger>
        <PopoverPortal>
          <PopoverContent
            class="issue-report-popover ui-surface"
            side="top"
            align="end"
            :side-offset="5"
            :collision-padding="8"
            aria-label="Report an Issue"
            :aria-busy="!!busy"
            @interact-outside="handleOutside"
            @escape-key-down="handleEscape"
            @keydown="handleKeydown"
          >
            <form
              class="issue-report-form"
              @submit.prevent="submit"
            >
              <template v-if="view === 'loading'">
                <div class="issue-report-loading">
                  <UiSpinner
                    v-if="showSpinner"
                    label="Loading connections"
                  />
                </div>
              </template>
              <template v-else-if="view === 'load-error'">
                <p
                  class="issue-report-error"
                  role="alert"
                >
                  {{ errors.load }}
                </p>
                <footer><UiButton @click="load">Retry</UiButton></footer>
              </template>
              <template v-else-if="view === 'report'">
                <h2>Report an Issue</h2>
                <label class="issue-report-field issue-report-description">
                  <span>Describe the Issue</span>
                  <UiContextMenu
                    :items="() => textFieldItems(() => descriptionInput?.input)"
                  >
                    <UiTextarea
                      ref="descriptionInput"
                      v-model="description"
                      rows="6"
                      maxlength="10000"
                      :readonly="!!busy"
                      aria-label="Describe the Issue"
                    />
                  </UiContextMenu>
                </label>
                <UiCheckbox
                  v-model="includeSession"
                  :disabled="!sessionId || !!busy"
                  >Include Current Session</UiCheckbox
                >
                <p
                  v-if="errors.send"
                  class="issue-report-error"
                  role="alert"
                >
                  {{ errors.send }}
                </p>
                <footer>
                  <UiButton
                    variant="ghost"
                    :disabled="!!busy"
                    @click="showConnections"
                    >Connections</UiButton
                  >
                  <UiButton
                    variant="primary"
                    type="submit"
                    :disabled="!canSubmit"
                    ><UiSpinner
                      v-if="showSpinner"
                      label="Sending report"
                    />Submit</UiButton
                  >
                </footer>
              </template>
              <template v-else>
                <header class="issue-report-provider">
                  <h2>
                    {{
                      view === 'connections'
                        ? 'Connections'
                        : view === 'grafana'
                          ? 'Grafana'
                          : 'Linear'
                    }}
                  </h2>
                  <UiIconButton
                    v-if="view === 'grafana' && connections.grafana"
                    size="xs"
                    label="Disconnect Grafana"
                    title="Disconnect Grafana"
                    :disabled="!!busy"
                    @click="disconnectGrafana"
                    ><UiIcon name="cross"
                  /></UiIconButton>
                </header>
                <p
                  v-if="reconnecting"
                  class="issue-report-note"
                >
                  Reconnect Linear to send reports.
                </p>
                <section v-if="view !== 'grafana'">
                  <header
                    v-if="view === 'connections'"
                    class="issue-report-provider"
                  >
                    <h3>Linear</h3>
                    <UiIconButton
                      v-if="connections.linear"
                      size="xs"
                      label="Disconnect Linear"
                      title="Disconnect Linear"
                      :disabled="!!busy"
                      @click="disconnectLinear"
                      ><UiIcon name="cross"
                    /></UiIconButton>
                  </header>
                  <p
                    v-if="connections.linear"
                    class="issue-report-summary"
                  >
                    {{ connections.linear.teamName
                    }}<template v-if="connections.linear.projectName">
                      / {{ connections.linear.projectName }}</template
                    >
                  </p>
                  <template v-else-if="!validated">
                    <label class="issue-report-field"
                      ><span>API Key</span
                      ><UiTokenInput
                        v-model="linearKey"
                        aria-label="API Key"
                        placeholder="Linear API key"
                        :readonly="!!busy"
                    /></label>
                    <p
                      v-if="errors.linear"
                      class="issue-report-error"
                      role="alert"
                    >
                      {{ errors.linear }}
                    </p>
                    <div class="issue-report-row">
                      <UiButton
                        :variant="
                          view === 'connections' ? 'secondary' : 'primary'
                        "
                        :disabled="!canValidate"
                        @click="validateLinear"
                        ><UiSpinner
                          v-if="showSpinner && busy === 'linear'"
                        />Connect</UiButton
                      >
                    </div>
                  </template>
                  <template v-else>
                    <div class="issue-report-destination">
                      <label class="issue-report-field"
                        ><span>Team</span
                        ><UiSelect
                          v-model="teamId"
                          :options="teamOptions"
                          aria-label="Team"
                          :disabled="!!busy"
                      /></label>
                      <label class="issue-report-field"
                        ><span>Project</span
                        ><UiSelect
                          v-model="projectId"
                          :options="projectOptions"
                          aria-label="Project"
                          :disabled="!!busy"
                      /></label>
                    </div>
                    <p
                      v-if="errors.destination"
                      class="issue-report-error"
                      role="alert"
                    >
                      {{ errors.destination }}
                    </p>
                    <div class="issue-report-row">
                      <UiButton
                        :variant="
                          view === 'connections' ? 'secondary' : 'primary'
                        "
                        :disabled="!canSaveDestination"
                        @click="saveLinear"
                        ><UiSpinner
                          v-if="showSpinner && busy === 'destination'"
                        />Save Destination</UiButton
                      >
                      <UiButton
                        v-if="view !== 'connections'"
                        variant="ghost"
                        :disabled="!!busy"
                        @click="backToKey"
                        >Back</UiButton
                      >
                    </div>
                  </template>
                  <p
                    v-if="errors['disconnect-linear']"
                    class="issue-report-error"
                    role="alert"
                  >
                    {{ errors['disconnect-linear'] }}
                  </p>
                </section>
                <section v-if="view === 'grafana' || view === 'connections'">
                  <header
                    v-if="view === 'connections'"
                    class="issue-report-provider"
                  >
                    <h3>
                      Grafana
                      <span class="issue-report-optional">Optional</span>
                    </h3>
                    <UiIconButton
                      v-if="connections.grafana"
                      size="xs"
                      label="Disconnect Grafana"
                      title="Disconnect Grafana"
                      :disabled="!!busy"
                      @click="disconnectGrafana"
                      ><UiIcon name="cross"
                    /></UiIconButton>
                  </header>
                  <span
                    v-else
                    class="issue-report-note"
                    >Optional</span
                  >
                  <p
                    v-if="connections.grafana"
                    class="issue-report-summary"
                  >
                    {{ connections.grafana.endpoint }}
                  </p>
                  <template v-else>
                    <label class="issue-report-field"
                      ><span>OTLP Endpoint</span
                      ><UiInput
                        v-model="endpoint"
                        placeholder="https://…/otlp"
                        :readonly="!!busy"
                        spellcheck="false"
                        autocomplete="off"
                    /></label>
                    <label class="issue-report-field"
                      ><span>Instance ID</span
                      ><UiInput
                        v-model="instanceId"
                        :readonly="!!busy"
                        spellcheck="false"
                        autocomplete="off"
                    /></label>
                    <label class="issue-report-field"
                      ><span>Token</span
                      ><UiTokenInput
                        v-model="token"
                        aria-label="Token"
                        :readonly="!!busy"
                    /></label>
                    <p
                      v-if="errors.grafana"
                      class="issue-report-error"
                      role="alert"
                    >
                      {{ errors.grafana }}
                    </p>
                    <div class="issue-report-row">
                      <UiButton
                        :variant="
                          view === 'connections' ? 'secondary' : 'primary'
                        "
                        :disabled="!canSaveGrafana"
                        @click="saveGrafana"
                        ><UiSpinner
                          v-if="showSpinner && busy === 'grafana'"
                        />Connect</UiButton
                      >
                    </div>
                  </template>
                  <p
                    v-if="errors['disconnect-grafana']"
                    class="issue-report-error"
                    role="alert"
                  >
                    {{ errors['disconnect-grafana'] }}
                  </p>
                </section>
                <footer v-if="view === 'grafana' || view === 'connections'">
                  <span />
                  <UiButton
                    :variant="
                      view === 'grafana' && !connections.grafana
                        ? 'ghost'
                        : 'primary'
                    "
                    :disabled="!!busy || !connections.linear"
                    @click="showReport"
                    >{{
                      view === 'connections'
                        ? 'Done'
                        : connections.grafana
                          ? 'Continue'
                          : 'Skip'
                    }}</UiButton
                  >
                </footer>
              </template>
            </form>
          </PopoverContent>
        </PopoverPortal>
      </PopoverRoot>
    </span>
  </UiTooltip>
</template>

<script setup lang="ts">
import {
  PopoverContent,
  PopoverPortal,
  PopoverRoot,
  PopoverTrigger,
} from 'reka-ui';
import { nextTick, onBeforeUnmount, ref, watch } from 'vue';

import { createIssueReporter } from '../composables/useIssueReporter';
import { preserveSurfaceUnderPalette } from '../lib/app-commands/surface';
import { issueReportingApi } from '../lib/cloud-connections';
import type { IssueReportingApi } from '../lib/cloud-types';
import textFieldItems from '../lib/text-menu';

import UiButton from './ui/UiButton.vue';
import UiCheckbox from './ui/UiCheckbox.vue';
import UiContextMenu from './ui/UiContextMenu.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiInput from './ui/UiInput.vue';
import UiSelect from './ui/UiSelect.vue';
import UiSpinner from './ui/UiSpinner.vue';
import UiTextarea from './ui/UiTextarea.vue';
import UiTokenInput from './ui/UiTokenInput.vue';
import UiTooltip from './ui/UiTooltip.vue';

const props = defineProps<{ sessionId?: string; api?: IssueReportingApi }>();
const reporter = createIssueReporter(
  props.api ?? issueReportingApi,
  () => props.sessionId,
);
const {
  open,
  view,
  connections,
  busy,
  showSpinner,
  errors,
  description,
  includeSession,
  linearKey,
  teamId,
  projectId,
  validated,
  endpoint,
  instanceId,
  token,
  reconnecting,
  teamOptions,
  projectOptions,
  canValidate,
  canSaveDestination,
  canSaveGrafana,
  canSubmit,
  openReporter,
  setOpen,
  load,
  showConnections,
  showReport,
  backToKey,
  validateLinear,
  saveLinear,
  saveGrafana,
  submit,
} = reporter;
const descriptionInput = ref<InstanceType<typeof UiTextarea>>();
let lastOwnedFocus: HTMLElement | undefined;

function disconnectLinear(): void {
  void reporter.disconnect('linear');
}
function disconnectGrafana(): void {
  void reporter.disconnect('grafana');
}
function handleEscape(event: Event): void {
  if (busy.value) event.preventDefault();
}
function handleOutside(event: CustomEvent<{ originalEvent: Event }>): void {
  preserveSurfaceUnderPalette(event);
  if (busy.value) event.preventDefault();
}
function handleKeydown(event: KeyboardEvent): void {
  if (event.isComposing || (!event.metaKey && !event.ctrlKey)) return;
  if (event.key === 'Enter') {
    event.preventDefault();
    event.stopPropagation();
    submit();
  } else if (
    event.key.toLowerCase() === 's' &&
    event.shiftKey &&
    view.value === 'report'
  ) {
    event.preventDefault();
    event.stopPropagation();
    if (props.sessionId && !busy.value)
      includeSession.value = !includeSession.value;
  }
}

// Repair focus after a replaced/disabled field, but never reclaim it from the palette.
watch([busy, view, validated, connections], () => {
  const surface = document.querySelector('.issue-report-popover');
  const active = document.activeElement;
  if (active instanceof HTMLElement && surface?.contains(active))
    lastOwnedFocus = active;
  void nextTick(() => {
    if (
      !open.value ||
      busy.value ||
      !surface ||
      document.querySelector('.command-palette') ||
      document.activeElement !== document.body ||
      !lastOwnedFocus
    )
      return;
    const target = lastOwnedFocus.isConnected
      ? lastOwnedFocus
      : surface.querySelector<HTMLElement>(
          'textarea:not(:disabled), input:not(:disabled), button:not(:disabled)',
        );
    target?.focus();
  });
});
onBeforeUnmount(reporter.dispose);
defineExpose({ openReporter });
</script>

<style scoped>
.issue-report-trigger {
  display: flex;
}

:global(.issue-report-popover) {
  position: relative;
  z-index: 20;
  width: min(320px, calc(100vw - 16px));
  max-height: min(calc(100vh - 16px), var(--reka-popper-available-height));
  padding: 10px;
  overflow: auto;
  outline: 0;
  overscroll-behavior: contain;
}

:global(body:has(.issue-report-popover) .ui-select-list) {
  z-index: 21;
}

.issue-report-form,
section {
  display: flex;
  flex-direction: column;
  gap: 9px;
}

h2,
h3 {
  margin: 0;
  color: var(--text);
  font-size: var(--text-sm);
  font-weight: 500;
  line-height: var(--leading-ui);
}

section + section {
  padding-top: 12px;
  border-top: 1px solid var(--border);
}

.issue-report-provider {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.issue-report-field {
  display: flex;
  flex-direction: column;
  min-width: 0;
  color: var(--muted);
  font-size: var(--text-xs);
  gap: 5px;
}

.issue-report-description :deep(.ui-textarea) {
  min-height: 108px;
}

.issue-report-destination {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.issue-report-destination :deep(.ui-select) {
  width: 100%;
}

.issue-report-row {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}

.issue-report-summary,
.issue-report-note,
.issue-report-error {
  margin: 0;
  font-size: var(--text-xs);
  line-height: var(--leading-ui);
}

.issue-report-summary {
  overflow-wrap: anywhere;
  color: var(--muted);
}

.issue-report-note,
.issue-report-optional {
  color: var(--muted);
}

.issue-report-optional {
  margin-left: 5px;
  font-size: var(--text-xs);
  font-weight: 400;
}

.issue-report-error {
  color: var(--danger);
}

.issue-report-loading {
  display: grid;
  min-height: 44px;
  place-items: center;
}

footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-top: 3px;
  gap: 8px;
}

.issue-report-form :deep(.ui-button) {
  gap: 6px;
}
</style>

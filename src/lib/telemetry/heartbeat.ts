/*
 * Stage 5's frontend liveness signal: a low-frequency heartbeat (visibility/
 * focus, event-loop lag, and coarse counts), a periodic sanitized state
 * summary, and the `PerformanceObserver` `longtask` observer where the
 * webview supports it. One self-rescheduling `setTimeout` loop drives both
 * the heartbeat and the state summary so they share an interval without two
 * competing `setInterval`s, and so the loop can measure its own scheduling
 * delay (the event-loop lag reading) directly. Called once from
 * `src/main.ts`, alongside `initTelemetry`/`installFrontendErrorCapture`.
 */
import { buildStateSnapshot } from '../../composables/state';
import { oldestPendingRpcAgeMs, pendingRpcCount } from '../pi/runtime';

import type { HeartbeatVisibility } from './attributes';

import {
  recordEventLoopLag,
  recordHeartbeat,
  recordLongTask,
  recordStateSummary,
  telemetryQueueLength,
} from './index';

const HEARTBEAT_INTERVAL_MS = 30_000;

let heartbeatTimer: ReturnType<typeof setTimeout> | undefined;

function currentVisibility(): HeartbeatVisibility {
  try {
    return document.visibilityState === 'hidden' ? 'hidden' : 'visible';
  } catch {
    return 'visible';
  }
}

function currentlyFocused(): boolean {
  try {
    return document.hasFocus();
  } catch {
    return true;
  }
}

function tick(expectedAt: number): void {
  try {
    const lagMs = Math.max(0, Date.now() - expectedAt);
    const visibility = currentVisibility();
    const focused = currentlyFocused();
    const snapshot = buildStateSnapshot();
    const pendingRpcs = pendingRpcCount();

    recordEventLoopLag(lagMs, visibility, focused);
    recordHeartbeat({
      visibility,
      focused,
      pendingRpcCount: pendingRpcs,
      controllerCount: snapshot.controllerCount,
      activeControllerCount: snapshot.activeControllerCount,
      runtimeCount: snapshot.runtimeCount,
      queueLength: telemetryQueueLength(),
    });
    recordStateSummary({
      controllerCount: snapshot.controllerCount,
      runtimeCount: snapshot.runtimeCount,
      pendingRpcCount: pendingRpcs,
      notificationCount: snapshot.notificationCount,
      dialogCount: snapshot.dialogCount,
      transcriptCounts: snapshot.transcriptCounts,
      draftBucket: snapshot.draftBucket,
      oldestPendingRpcAgeMs: oldestPendingRpcAgeMs(),
      scope: snapshot.scope,
    });
  } catch {
    // A heartbeat failure must never crash the timer loop or the app.
  }
  scheduleNext();
}

function scheduleNext(): void {
  const expectedAt = Date.now() + HEARTBEAT_INTERVAL_MS;
  heartbeatTimer = setTimeout(() => tick(expectedAt), HEARTBEAT_INTERVAL_MS);
}

function startHeartbeat(): void {
  if (heartbeatTimer) return;
  scheduleNext();
}

function stopHeartbeat(): void {
  if (heartbeatTimer) clearTimeout(heartbeatTimer);
  heartbeatTimer = undefined;
}

let longTaskObserver: PerformanceObserver | undefined;

/**
 * Installs a `PerformanceObserver` for `longtask` entries, where the
 * webview supports them (still not universal). Each entry's duration is
 * recorded as a native OTel histogram, never a log — a stalled webview can
 * generate many of these, and a histogram aggregates client-side instead of
 * producing one persisted line per task. Safe to call more than once and
 * safe when unsupported: both are a no-op.
 */
function installLongTaskObserver(): void {
  try {
    if (longTaskObserver) return;
    if (typeof PerformanceObserver === 'undefined') return;
    if (!PerformanceObserver.supportedEntryTypes?.includes('longtask')) {
      return;
    }
    const observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        recordLongTask(entry.duration);
      }
    });
    observer.observe({ entryTypes: ['longtask'] });
    longTaskObserver = observer;
  } catch {
    // Long-task observation is best-effort; it must never fail startup.
  }
}

function stopLongTaskObserver(): void {
  longTaskObserver?.disconnect();
  longTaskObserver = undefined;
}

export {
  installLongTaskObserver,
  startHeartbeat,
  stopHeartbeat,
  stopLongTaskObserver,
};

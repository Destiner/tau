import { getCurrentWindow } from '@tauri-apps/api/window';
import { createApp } from 'vue';

import App from './App.vue';
import {
  initTelemetry,
  installFrontendErrorCapture,
  vueErrorHandler,
} from './lib/telemetry';
import {
  installLongTaskObserver,
  startHeartbeat,
} from './lib/telemetry/heartbeat';
import '@fontsource-variable/inter/wght.css';
import '@fontsource-variable/inter/wght-italic.css';
import './styles.css';
import './components/ui/surface.css';

// All four are synchronous, do no I/O, and never throw, so none delays
// window reveal or mounting below. `startHeartbeat`/`installLongTaskObserver`
// only arm a timer/observer here; the first tick and any long task are still
// asynchronous.
initTelemetry();
installFrontendErrorCapture();
startHeartbeat();
installLongTaskObserver();

async function mountApp(): Promise<void> {
  const fixture = new URLSearchParams(window.location.search).get('fixture');
  if (import.meta.env.DEV && fixture === 'long-transcript') {
    const { default: TranscriptFixture } =
      await import('./dev/TranscriptFixture.vue');
    const app = createApp(TranscriptFixture);
    app.config.errorHandler = vueErrorHandler;
    app.mount('#app');
    return;
  }
  if (import.meta.env.DEV && fixture === 'issue-report') {
    const { default: IssueReportFixture } =
      await import('./dev/IssueReportFixture.vue');
    const app = createApp(IssueReportFixture);
    app.config.errorHandler = vueErrorHandler;
    app.mount('#app');
    return;
  }
  if (import.meta.env.DEV && fixture === 'extension-dialog') {
    const { default: ExtensionDialogFixture } =
      await import('./dev/ExtensionDialogFixture.vue');
    const app = createApp(ExtensionDialogFixture);
    app.config.errorHandler = vueErrorHandler;
    app.mount('#app');
    return;
  }

  if (import.meta.env.DEV && fixture === 'remote-dialog') {
    const { default: RemoteDialogFixture } =
      await import('./dev/RemoteDialogFixture.vue');
    const app = createApp(RemoteDialogFixture);
    app.config.errorHandler = vueErrorHandler;
    app.mount('#app');
    return;
  }

  if (import.meta.env.DEV) {
    const testScenario = new URLSearchParams(window.location.search).get(
      'test-scenario',
    );
    if (testScenario) {
      const { default: installPiScenarioAdapter } =
        await import('./dev/pi-scenario-adapter');
      installPiScenarioAdapter(testScenario);
    }
  }

  const app = createApp(App);
  app.config.errorHandler = vueErrorHandler;
  app.mount('#app');
}

/**
 * The window is created hidden so that no empty frame is ever on screen, and is
 * shown once the app has mounted the DOM it is about to draw.
 *
 * Nothing may be awaited before the call: a hidden webview has both its frames
 * and its timers throttled, so a wait on either can fail to resume and would
 * leave the window hidden for the rest of the run.
 */
function revealWindow(): void {
  try {
    // `getCurrentWindow()` itself throws synchronously (not a rejected
    // promise) outside a real Tauri webview — a plain browser (a Playwright
    // test, `vite dev` in a tab) has no `window.__TAURI_INTERNALS__` for it
    // to read. The whole call is wrapped, not just `.show()`'s rejection, so
    // that case degrades the same way: log and move on, never an uncaught
    // page error.
    getCurrentWindow()
      .show()
      .catch((error: unknown) => {
        // Expected in a plain browser, which has no window to show. Anything else
        // left the window hidden, which is worth seeing in the console.
        console.debug('Could not show the window', error);
      });
  } catch (error) {
    console.debug('Could not show the window', error);
  }
}

void mountApp().finally(revealWindow);

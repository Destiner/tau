import { getCurrentWindow } from '@tauri-apps/api/window';
import { createApp } from 'vue';

import App from './App.vue';
import '@fontsource-variable/inter/wght.css';
import './styles.css';

async function mountApp(): Promise<void> {
  const fixture = new URLSearchParams(window.location.search).get('fixture');
  if (import.meta.env.DEV && fixture === 'long-transcript') {
    const { default: TranscriptFixture } =
      await import('./dev/TranscriptFixture.vue');
    createApp(TranscriptFixture).mount('#app');
    return;
  }

  createApp(App).mount('#app');
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
  getCurrentWindow()
    .show()
    .catch((error: unknown) => {
      // Expected in a plain browser, which has no window to show. Anything else
      // left the window hidden, which is worth seeing in the console.
      console.debug('Could not show the window', error);
    });
}

void mountApp().finally(revealWindow);

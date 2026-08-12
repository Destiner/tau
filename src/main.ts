import { createApp } from "vue";
import App from "./App.vue";
import "./styles.css";

async function mountApp() {
  const fixture = new URLSearchParams(window.location.search).get("fixture");
  if (import.meta.env.DEV && fixture === "long-transcript") {
    const { default: TranscriptFixture } =
      await import("./dev/TranscriptFixture.vue");
    createApp(TranscriptFixture).mount("#app");
    return;
  }

  createApp(App).mount("#app");
}

void mountApp();

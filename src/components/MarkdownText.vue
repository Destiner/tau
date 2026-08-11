<script setup lang="ts">
import { openUrl } from "@tauri-apps/plugin-opener";
import { computed } from "vue";
import { isWebUrl, renderMarkdown } from "../lib/markdown";

const props = defineProps<{ source: string }>();
const rendered = computed(() => renderMarkdown(props.source));

async function handleClick(event: MouseEvent): Promise<void> {
  const target = event.target;
  if (!(target instanceof Element)) return;

  const link = target.closest("a[href]");
  if (!(link instanceof HTMLAnchorElement) || !isWebUrl(link.href)) return;

  event.preventDefault();

  try {
    await openUrl(link.href);
  } catch (error) {
    console.error("Could not open link in the default browser", error);
  }
}
</script>

<template>
  <div class="markdown" @click="handleClick" v-html="rendered"></div>
</template>

import { shallowRef } from 'vue';

interface QueuePreviewControls {
  advance: () => Promise<void>;
  status: () => string;
}

const queuePreviewControls = shallowRef<QueuePreviewControls>();

function setQueuePreviewControls(controls: QueuePreviewControls): void {
  queuePreviewControls.value = controls;
}

export { queuePreviewControls, setQueuePreviewControls };
export type { QueuePreviewControls };

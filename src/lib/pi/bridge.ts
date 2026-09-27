export interface PiBridgeEvent {
  runtimeId: string;
  generation: number;
  kind: 'started' | 'rpc' | 'stderr' | 'error' | 'exited';
  line?: string;
  message?: string;
  code?: number;
}

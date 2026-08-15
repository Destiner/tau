/** An event from a Pi runtime bridge, emitted as its process runs. */
export interface PiBridgeEvent {
  runtimeId: string;
  generation: number;
  kind: "started" | "rpc" | "stderr" | "error" | "exited";
  line?: string;
  message?: string;
  code?: number;
}

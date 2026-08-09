export interface WorkspaceSnapshot {
  activeProjectPath: string;
  piPath: string | null;
  sdkAvailable: boolean;
  projects: ProjectSummary[];
}

export type IntegrationKind = "rpc" | "sdk";

export interface ProjectSummary {
  path: string;
  name: string;
  collapsed: boolean;
  selected: boolean;
  sessions: SessionSummary[];
}

export interface SessionSummary {
  id: string;
  path: string;
  title: string;
  lastActive: string;
  archived: boolean;
  selected: boolean;
}

export interface ModelOption {
  provider: string;
  id: string;
  name: string;
  reasoning: boolean;
}

export type ThinkingLevel =
  "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";

export interface TranscriptEntry {
  id: string;
  kind: "user" | "assistant" | "thinking" | "tool";
  text: string;
  toolCallId?: string;
  toolName?: string;
  toolRunning?: boolean;
  toolErrored?: boolean;
}

export interface PiBridgeEvent {
  generation: number;
  kind: "started" | "rpc" | "stderr" | "error" | "exited";
  line?: string;
  message?: string;
  code?: number;
}

export interface WorkspaceSnapshot {
  activeProjectPath: string;
  piPath: string | null;
  projects: ProjectSummary[];
}

export interface ProjectSummary {
  path: string;
  name: string;
  workingDirectory: string;
  connectionString?: string;
  collapsed: boolean;
  selected: boolean;
  sessions: SessionSummary[];
}

export interface RemoteDirectoryEntry {
  name: string;
  path: string;
}

export interface RemoteDirectoryListing {
  connectionString: string;
  workingDirectory: string;
  host: string;
  directories: RemoteDirectoryEntry[];
}

export interface SessionSummary {
  id: string;
  path: string;
  title: string;
  lastActive: string;
  lastUserMessageAt: number;
  archived: boolean;
  selected: boolean;
}

export interface ModelOption {
  provider: string;
  id: string;
  name: string;
  reasoning: boolean;
}

export type CommandSource = "extension" | "prompt" | "skill";

export interface CommandOption {
  name: string;
  description?: string;
  source: CommandSource;
  sourceInfo?: {
    path: string;
    source: string;
    scope: "user" | "project" | "temporary";
    origin: "package" | "top-level";
    baseDir?: string;
  };
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
  toolArguments?: string;
  toolResult?: string;
}

export interface PiBridgeEvent {
  runtimeId: string;
  generation: number;
  kind: "started" | "rpc" | "stderr" | "error" | "exited";
  line?: string;
  message?: string;
  code?: number;
}

export type ExtensionDialogMethod = "select" | "confirm" | "input" | "editor";

export interface ExtensionDialog {
  key: string;
  requestId: string;
  method: ExtensionDialogMethod;
  title: string;
  message?: string;
  options?: string[];
  placeholder?: string;
  prefill?: string;
  draft: string;
  timeout?: number;
  controllerKey: string;
  runtimeId: string;
  generation: number;
  projectName: string;
  sessionName: string;
}

export type ExtensionNotificationType = "info" | "warning" | "error";

export interface ExtensionNotification {
  key: string;
  message: string;
  type: ExtensionNotificationType;
  projectName: string;
  sessionName: string;
}

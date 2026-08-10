import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const sidecarPath = fileURLToPath(new URL("./pi-sdk.mjs", import.meta.url));
const projectPath = "/tmp/tau project";

const sdkFixture = `
const calls = [];

function manager(kind, cwd, path) {
  return { kind, cwd, path };
}

export function getAgentDir() {
  calls.push({ name: "getAgentDir" });
  return "/tmp/pi-agent";
}

export const SessionManager = {
  create(cwd) {
    calls.push({ name: "SessionManager.create", cwd });
    return manager("create", cwd);
  },
  open(path, sessionDir, cwdOverride) {
    calls.push({
      name: "SessionManager.open",
      path,
      sessionDir: sessionDir ?? null,
      cwdOverride,
    });
    return manager("open", cwdOverride, path);
  },
};

export async function createAgentSessionServices(options) {
  calls.push({ name: "createAgentSessionServices", options });
  return { kind: "services", diagnostics: ["fixture diagnostic"], ...options };
}

export async function createAgentSessionFromServices(options) {
  calls.push({
    name: "createAgentSessionFromServices",
    servicesKind: options.services.kind,
    sessionManager: options.sessionManager,
    sessionStartEvent: options.sessionStartEvent,
  });
  return { session: { kind: "session" } };
}

export async function createAgentSessionRuntime(factory, options) {
  calls.push({
    name: "createAgentSessionRuntime",
    cwd: options.cwd,
    agentDir: options.agentDir,
    sessionManager: options.sessionManager,
  });
  const replacementManager = manager("replacement", "/tmp/replacement");
  const created = await factory({
    cwd: "/tmp/replacement",
    agentDir: options.agentDir,
    sessionManager: replacementManager,
    sessionStartEvent: {
      type: "session_start",
      reason: "new",
      previousSessionFile: "/tmp/previous.jsonl",
    },
  });
  return { kind: "runtime", created };
}

export async function runRpcMode(runtime) {
  calls.push({
    name: "runRpcMode",
    runtimeKind: runtime.kind,
    diagnostics: runtime.created.diagnostics,
  });
  process.stdout.write(JSON.stringify(calls) + "\\n");
}
`;

describe("Pi SDK sidecar", () => {
  it.each([
    {
      name: "creates a persistent session",
      extraArgs: [],
      managerCall: {
        name: "SessionManager.create",
        cwd: projectPath,
      },
      manager: { kind: "create", cwd: projectPath },
    },
    {
      name: "opens the requested session in Tau's project",
      extraArgs: ["--session", "/tmp/session.jsonl"],
      managerCall: {
        name: "SessionManager.open",
        path: "/tmp/session.jsonl",
        sessionDir: null,
        cwdOverride: projectPath,
      },
      manager: {
        kind: "open",
        cwd: projectPath,
        path: "/tmp/session.jsonl",
      },
    },
  ])(
    "$name and delegates RPC to Pi",
    async ({ extraArgs, managerCall, manager }) => {
      const directory = await mkdtemp(join(tmpdir(), "tau-sdk-sidecar-"));
      const sdkEntry = join(directory, "sdk-fixture.mjs");
      await writeFile(sdkEntry, sdkFixture);

      try {
        const result = await runSidecar([
          "--sdk-entry",
          sdkEntry,
          "--cwd",
          projectPath,
          ...extraArgs,
        ]);
        expect(result).toMatchObject({ code: 0, stderr: "" });

        const calls = JSON.parse(result.stdout.trim());
        expect(calls).toContainEqual({ name: "getAgentDir" });
        expect(calls).toContainEqual(managerCall);
        expect(calls).toContainEqual({
          name: "createAgentSessionRuntime",
          cwd: projectPath,
          agentDir: "/tmp/pi-agent",
          sessionManager: manager,
        });
        expect(calls).toContainEqual({
          name: "createAgentSessionServices",
          options: { cwd: "/tmp/replacement", agentDir: "/tmp/pi-agent" },
        });
        expect(calls).toContainEqual({
          name: "createAgentSessionFromServices",
          servicesKind: "services",
          sessionManager: {
            kind: "replacement",
            cwd: "/tmp/replacement",
          },
          sessionStartEvent: {
            type: "session_start",
            reason: "new",
            previousSessionFile: "/tmp/previous.jsonl",
          },
        });
        expect(calls.at(-1)).toEqual({
          name: "runRpcMode",
          runtimeKind: "runtime",
          diagnostics: ["fixture diagnostic"],
        });
      } finally {
        await rm(directory, { recursive: true, force: true });
      }
    },
  );
});

function runSidecar(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [sidecarPath, ...args], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

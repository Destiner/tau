import type { IntegrationKind } from "../types";

interface PiIntegration {
  kind: IntegrationKind;
  startCommand: "start_pi" | "start_pi_sdk";
  requiresSdk: boolean;
}

const integrations: Record<IntegrationKind, PiIntegration> = {
  rpc: {
    kind: "rpc",
    startCommand: "start_pi",
    requiresSdk: false,
  },
  sdk: {
    kind: "sdk",
    startCommand: "start_pi_sdk",
    requiresSdk: true,
  },
};

export const activePiIntegration: IntegrationKind = "rpc";

export function getActivePiIntegration(): PiIntegration {
  return integrations[activePiIntegration];
}

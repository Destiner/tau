import { type TelemetryScope } from '../../lib/telemetry';
import { type SessionController } from '../state';

function controllerTelemetryScope(
  controller: SessionController | undefined,
  sessionId?: string,
): TelemetryScope {
  return {
    sessionId: controller?.sessionId || sessionId,
    controllerId: controller?.key,
    runtimeId: controller?.runtimeId,
    generation: controller?.generation || undefined,
  };
}

export default controllerTelemetryScope;

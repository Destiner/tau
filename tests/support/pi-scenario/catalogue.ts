import savedSessionConversation from './saved-session-conversation';

import type { PiScenario } from './index';

function findPiScenario(name: string): PiScenario | undefined {
  return name === savedSessionConversation.metadata.name
    ? savedSessionConversation
    : undefined;
}

export { findPiScenario };

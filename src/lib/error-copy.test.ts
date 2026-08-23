import { describe, expect, it } from 'vitest';

import { errorCopy, rpcFailureCopy } from './error-copy';

describe('operational error copy', () => {
  it('maps RPC failures to the operation the user attempted', () => {
    expect(rpcFailureCopy('prompt')).toBe(errorCopy.messageSend);
    expect(rpcFailureCopy('abort')).toBe(errorCopy.stopWork);
    expect(rpcFailureCopy('set_model')).toBe(errorCopy.modelChange);
    expect(rpcFailureCopy('set_thinking_level')).toBe(errorCopy.effortChange);
    expect(rpcFailureCopy('set_session_name')).toBe(errorCopy.sessionRename);
    expect(rpcFailureCopy('extension_ui_response')).toBe(
      errorCopy.extensionResponse,
    );
  });

  it('does not expose an unknown command or rejection payload', () => {
    const canary = 'get_secret: /Users/tau/project TOKEN_CANARY';
    const copy = rpcFailureCopy(canary);

    expect(copy).toBe(errorCopy.sessionRefresh);
    expect(copy).not.toContain(canary);
  });
});

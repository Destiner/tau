import { describe, expect, it } from 'vitest';

import { errorCopy, feedbackTitle, rpcFailureCopy } from './error-copy';

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

  it('uses operation-specific fallback headings', () => {
    expect(feedbackTitle(errorCopy.messageSend)).toBe('Message Not Sent');
    expect(feedbackTitle(errorCopy.piOwnership)).toBe('Pi Unavailable');
    expect(feedbackTitle('The connection was lost.')).toBe(
      'Remote Connection Lost',
    );
  });

  it('does not expose an unknown command or rejection payload', () => {
    const canary = 'get_secret: /Users/tau/project TOKEN_CANARY';
    const copy = rpcFailureCopy(canary);

    expect(copy).toBe(errorCopy.sessionRefresh);
    expect(copy).not.toContain(canary);
  });
});

import { describe, expect, it } from 'vitest';

import { describePiError, retryPiErrorMessage } from './error';

describe('Pi error copy', () => {
  it.each([
    [
      `401: {"message":"Incorrect API key provided."}`,
      'authentication',
      'The model provider could not authenticate this request. Check the provider credentials and try again.',
    ],
    [
      `402: {"message":"This request requires more credits."}`,
      'credit',
      'The account has no available credit for this request. Add credit or choose another model, then try again.',
    ],
    [
      `429: {"error":{"code":"insufficient_quota"}}`,
      'credit',
      'The account has no available credit for this request. Add credit or choose another model, then try again.',
    ],
    [
      '429: rate limit quota exceeded',
      'rate-limit',
      'The model provider is receiving too many requests. Wait a moment or choose another model, then try again.',
    ],
    [
      'Maximum context length exceeded',
      'context-limit',
      'This conversation is too long for the selected model. Start a new session or choose a model with a larger context window.',
    ],
    [
      'The selected model is not available for this account',
      'model-unavailable',
      'The selected model is unavailable to this account. Choose another model and try again.',
    ],
    [
      'TypeError: fetch failed',
      'network',
      'The model provider could not be reached. Check the connection and try again.',
    ],
    [
      `529 {"error":{"message":"Overloaded"}}`,
      'unavailable',
      'The model provider is temporarily unavailable. Wait a moment or choose another model, then try again.',
    ],
    [
      '400: invalid request',
      'invalid-request',
      'The model provider could not process this request. Edit it or choose another model.',
    ],
  ] as const)('maps %s to reviewed copy', (raw, kind, message) => {
    expect(describePiError(raw)).toEqual({
      kind,
      label: 'Reply Failed',
      message,
    });
  });

  it('uses fixed fallback copy without exposing raw technical details', () => {
    const canary =
      '500 RAW_PAYLOAD_CANARY\n at run (/Users/tau/project/index.ts:4:2)';
    const description = describePiError(canary);

    expect(description).toEqual({
      kind: 'unknown',
      label: 'Reply Failed',
      message: 'The reply failed. Try again or choose another model.',
    });
    expect(JSON.stringify(description)).not.toContain('RAW_PAYLOAD_CANARY');
    expect(JSON.stringify(description)).not.toContain('/Users/tau');
  });

  it('uses terse reviewed reasons while retrying', () => {
    expect(retryPiErrorMessage('rate-limit')).toBe(
      'The model provider is receiving too many requests.',
    );
    expect(retryPiErrorMessage('unavailable')).toBe(
      'The model provider is temporarily unavailable.',
    );
    expect(retryPiErrorMessage('network')).toBe(
      'The model provider could not be reached.',
    );
    expect(retryPiErrorMessage('authentication')).toBe('The reply failed.');
  });
});

import { describe, expect, it } from 'vitest';

import {
  classifyErrorKind,
  locationFromErrorEvent,
  locationFromStack,
  locationFromValue,
} from './errors';
import { FORBIDDEN_CONTENT_CANARIES } from './privacy';

describe('classifyErrorKind', () => {
  it('classifies built-in and unknown thrown values without leaking messages', () => {
    expect(classifyErrorKind(new TypeError('x'))).toBe('TypeError');
    expect(classifyErrorKind(new RangeError('x'))).toBe('RangeError');
    expect(classifyErrorKind(new ReferenceError('x'))).toBe('ReferenceError');
    expect(classifyErrorKind(new SyntaxError('x'))).toBe('SyntaxError');
    expect(classifyErrorKind(new EvalError('x'))).toBe('EvalError');
    expect(classifyErrorKind(new URIError('x'))).toBe('URIError');
    expect(classifyErrorKind(new Error('x'))).toBe('Error');
    class CustomError extends Error {
      constructor() {
        super('x');
        this.name = 'CustomError';
      }
    }
    expect(classifyErrorKind(new CustomError())).toBe('other');
    expect(classifyErrorKind('a string reason')).toBe('other');
    expect(classifyErrorKind({ message: 'plain object' })).toBe('other');
    expect(classifyErrorKind(42)).toBe('other');
    expect(classifyErrorKind(null)).toBe('other');
    expect(classifyErrorKind(undefined)).toBe('none');
    const canary = FORBIDDEN_CONTENT_CANARIES.get('prompt') ?? '';
    const error = new TypeError(canary);
    expect(classifyErrorKind(error)).toBe('TypeError');
  });
});

describe('locationFromStack', () => {
  it('extracts a V8 frame, fails closed on unknown shapes and removes paths', () => {
    const stack =
      'TypeError: boom\n    at doWork (/Users/tau/project/src/foo.ts:12:5)\n    at main (/Users/tau/project/src/index.ts:3:1)';
    expect(locationFromStack(stack)).toBe('foo.ts:12:5');
    expect(locationFromStack('nothing frame-shaped here')).toBe('');
    const canaryPath = FORBIDDEN_CONTENT_CANARIES.get('projectPath') ?? '';
    const canaryStack = `Error: boom\n    at run (${canaryPath}/src/index.ts:1:1)`;
    const location = locationFromStack(canaryStack);
    expect(location).toBe('index.ts:1:1');
    expect(location).not.toContain(canaryPath);
  });
});

describe('locationFromValue', () => {
  it('reads Error stacks but not other values', () => {
    const error = new Error('boom');
    error.stack = 'Error: boom\n    at run (/tmp/project/src/a.ts:7:2)';
    expect(locationFromValue(error)).toBe('a.ts:7:2');
    expect(locationFromValue('a plain string reason')).toBe('');
    expect(locationFromValue(undefined)).toBe('');
  });
});

describe('locationFromErrorEvent', () => {
  it('prefers structured coordinates, falls back to stacks, and fails closed', () => {
    const error = new Error('boom');
    error.stack = 'Error: boom\n    at run (/tmp/other.ts:9:9)';
    const location = locationFromErrorEvent({
      filename: '/tmp/project/src/b.ts',
      lineno: 4,
      colno: 8,
      error,
    });
    expect(location).toBe('b.ts:4:8');
    error.stack = 'Error: boom\n    at run (/tmp/project/src/c.ts:2:3)';
    expect(locationFromErrorEvent({ error })).toBe('c.ts:2:3');
    expect(locationFromErrorEvent({})).toBe('');
  });
});

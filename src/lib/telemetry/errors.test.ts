import { describe, expect, it } from 'vitest';

import {
  classifyErrorKind,
  locationFromErrorEvent,
  locationFromStack,
  locationFromValue,
} from './errors';
import { FORBIDDEN_CONTENT_CANARIES } from './privacy';

describe('classifyErrorKind', () => {
  it('recognizes every built-in error constructor by name', () => {
    expect(classifyErrorKind(new TypeError('x'))).toBe('TypeError');
    expect(classifyErrorKind(new RangeError('x'))).toBe('RangeError');
    expect(classifyErrorKind(new ReferenceError('x'))).toBe('ReferenceError');
    expect(classifyErrorKind(new SyntaxError('x'))).toBe('SyntaxError');
    expect(classifyErrorKind(new EvalError('x'))).toBe('EvalError');
    expect(classifyErrorKind(new URIError('x'))).toBe('URIError');
    expect(classifyErrorKind(new Error('x'))).toBe('Error');
  });

  it('falls back to other for a subclassed or unrecognized error name', () => {
    class CustomError extends Error {
      constructor() {
        super('x');
        this.name = 'CustomError';
      }
    }
    expect(classifyErrorKind(new CustomError())).toBe('other');
  });

  it('falls back to other for a non-Error thrown/rejected value', () => {
    expect(classifyErrorKind('a string reason')).toBe('other');
    expect(classifyErrorKind({ message: 'plain object' })).toBe('other');
    expect(classifyErrorKind(42)).toBe('other');
    expect(classifyErrorKind(null)).toBe('other');
  });

  it('reports none when there is no value at all', () => {
    expect(classifyErrorKind(undefined)).toBe('none');
  });

  it('never reflects the error message into the kind', () => {
    const canary = FORBIDDEN_CONTENT_CANARIES.get('prompt') ?? '';
    const error = new TypeError(canary);
    expect(classifyErrorKind(error)).toBe('TypeError');
  });
});

describe('locationFromStack', () => {
  it('extracts and sanitizes the first frame of a V8-style stack', () => {
    const stack =
      'TypeError: boom\n    at doWork (/Users/tau/project/src/foo.ts:12:5)\n    at main (/Users/tau/project/src/index.ts:3:1)';
    expect(locationFromStack(stack)).toBe('foo.ts:12:5');
  });

  it('returns an empty string for an unfamiliar stack shape', () => {
    expect(locationFromStack('nothing frame-shaped here')).toBe('');
  });

  it('never leaks a directory component from the stack', () => {
    const canaryPath = FORBIDDEN_CONTENT_CANARIES.get('projectPath') ?? '';
    const stack = `Error: boom\n    at run (${canaryPath}/src/index.ts:1:1)`;
    const location = locationFromStack(stack);
    expect(location).toBe('index.ts:1:1');
    expect(location).not.toContain(canaryPath);
  });
});

describe('locationFromValue', () => {
  it('reads the location from an Error value', () => {
    const error = new Error('boom');
    error.stack = 'Error: boom\n    at run (/tmp/project/src/a.ts:7:2)';
    expect(locationFromValue(error)).toBe('a.ts:7:2');
  });

  it('is empty for a non-Error value', () => {
    expect(locationFromValue('a plain string reason')).toBe('');
    expect(locationFromValue(undefined)).toBe('');
  });
});

describe('locationFromErrorEvent', () => {
  it('prefers the structured filename/lineno/colno over the stack', () => {
    const error = new Error('boom');
    error.stack = 'Error: boom\n    at run (/tmp/other.ts:9:9)';
    const location = locationFromErrorEvent({
      filename: '/tmp/project/src/b.ts',
      lineno: 4,
      colno: 8,
      error,
    });
    expect(location).toBe('b.ts:4:8');
  });

  it('falls back to the error value when filename is unavailable', () => {
    const error = new Error('boom');
    error.stack = 'Error: boom\n    at run (/tmp/project/src/c.ts:2:3)';
    expect(locationFromErrorEvent({ error })).toBe('c.ts:2:3');
  });

  it('is empty when neither a filename nor an error value is available', () => {
    expect(locationFromErrorEvent({})).toBe('');
  });
});

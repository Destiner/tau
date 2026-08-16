import { describe, expect, it } from 'vitest';

import {
  allowedAttribute,
  type AttributeSpec,
  CONTEXT_ATTRIBUTES,
  FAMILIES,
  isMetricSafe,
  RESOURCE_ATTRIBUTES,
  validateAttribute,
} from './attributes';
import { DEFAULT_MAX_ATTRIBUTE_LEN } from './privacy';

function allSpecs(): AttributeSpec[] {
  return [
    ...RESOURCE_ATTRIBUTES,
    ...CONTEXT_ATTRIBUTES,
    ...FAMILIES.flatMap((family) => family.attributes),
  ];
}

describe('attribute catalog', () => {
  it('gives every string spec a max length and every int spec none', () => {
    for (const spec of allSpecs()) {
      if (spec.kind === 'string') {
        expect(spec.maxLen).not.toBeNull();
      } else {
        expect(spec.maxLen).toBeNull();
      }
    }
  });

  it('never marks a context identifier metric-safe', () => {
    for (const spec of CONTEXT_ATTRIBUTES) {
      expect(spec.metricSafe).toBe(false);
    }
  });

  it('resolves family attributes by name', () => {
    const spec = allowedAttribute('pi.rpc', 'pi.rpc.method');
    expect(spec?.kind).toBe('string');
  });

  it('resolves shared context attributes from every family', () => {
    const spec = allowedAttribute('ui.action', 'tau.session.id');
    expect(spec?.metricSafe).toBe(false);
  });

  it('rejects an unknown family or key', () => {
    expect(
      allowedAttribute('does.not.exist', 'tau.session.id'),
    ).toBeUndefined();
    expect(allowedAttribute('ui.action', 'tau.does.not.exist')).toBeUndefined();
  });
});

describe('validateAttribute', () => {
  it('accepts a well-formed attribute', () => {
    expect(validateAttribute('pi.rpc', 'pi.rpc.method', 'get_state')).toEqual({
      valid: true,
    });
    expect(
      validateAttribute('pi.process.lifecycle', 'tau.process.exit_code', 0),
    ).toEqual({ valid: true });
  });

  it('rejects an unknown attribute', () => {
    expect(validateAttribute('pi.rpc', 'pi.rpc.transcript', 'hello')).toEqual({
      valid: false,
      error: 'unknown-attribute',
    });
  });

  it('rejects a type mismatch', () => {
    expect(validateAttribute('pi.rpc', 'pi.rpc.method', 1)).toEqual({
      valid: false,
      error: 'wrong-type',
    });
    expect(
      validateAttribute(
        'pi.process.lifecycle',
        'tau.process.exit_code',
        'zero',
      ),
    ).toEqual({ valid: false, error: 'wrong-type' });
  });

  it('rejects a non-integer number for an int attribute', () => {
    expect(
      validateAttribute('pi.process.lifecycle', 'tau.process.exit_code', 1.5),
    ).toEqual({ valid: false, error: 'wrong-type' });
  });

  it('rejects a string oversized in UTF-8 bytes', () => {
    const ascii = 'x'.repeat(DEFAULT_MAX_ATTRIBUTE_LEN + 1);
    const multibyte = 'é'.repeat(DEFAULT_MAX_ATTRIBUTE_LEN / 2 + 1);
    for (const oversized of [ascii, multibyte]) {
      expect(validateAttribute('pi.rpc', 'pi.rpc.method', oversized)).toEqual({
        valid: false,
        error: 'too-long',
      });
    }
  });
});

describe('isMetricSafe', () => {
  it('matches the catalog', () => {
    expect(isMetricSafe('tau.action.name')).toBe(true);
    expect(isMetricSafe('tau.session.id')).toBe(false);
    expect(isMetricSafe('unknown.key')).toBe(false);
  });
});

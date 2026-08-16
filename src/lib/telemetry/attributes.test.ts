import { describe, expect, it } from 'vitest';

import {
  allowedAttribute,
  type AttributeSpec,
  CONTEXT_ATTRIBUTES,
  FAMILIES,
  isMetricSafe,
  PI_PROCESS_RESOLUTIONS,
  PI_PROCESS_STOP_REASONS,
  PI_RPC_METHODS,
  PI_RPC_OUTCOMES,
  RESOURCE_ATTRIBUTES,
  TAURI_INVOKE_COMMANDS,
  UI_ACTION_NAMES,
  validateAttribute,
} from './attributes';
import {
  DEFAULT_MAX_ATTRIBUTE_LEN,
  FORBIDDEN_CONTENT_CANARIES,
} from './privacy';

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

  it('rejects an unreviewed categorical value', () => {
    expect(
      validateAttribute('tauri.invoke', 'tau.invoke.command', 'user content'),
    ).toEqual({ valid: false, error: 'unknown-value' });
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

  it('rejects a non-integer or unsafe number for an int attribute', () => {
    for (const value of [1.5, Number.MAX_SAFE_INTEGER + 1]) {
      expect(
        validateAttribute(
          'pi.process.lifecycle',
          'tau.process.exit_code',
          value,
        ),
      ).toEqual({ valid: false, error: 'wrong-type' });
    }
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

  it('accepts every reviewed action name', () => {
    for (const name of UI_ACTION_NAMES) {
      expect(validateAttribute('ui.action', 'tau.action.name', name)).toEqual({
        valid: true,
      });
    }
  });

  it('rejects an unreviewed action name', () => {
    expect(
      validateAttribute('ui.action', 'tau.action.name', 'not.a.real.action'),
    ).toEqual({ valid: false, error: 'unknown-value' });
  });

  it('accepts every reviewed invoke command', () => {
    for (const command of TAURI_INVOKE_COMMANDS) {
      expect(
        validateAttribute('tauri.invoke', 'tau.invoke.command', command),
      ).toEqual({ valid: true });
    }
  });

  it('accepts every reviewed rpc method and outcome', () => {
    for (const method of PI_RPC_METHODS) {
      expect(validateAttribute('pi.rpc', 'pi.rpc.method', method)).toEqual({
        valid: true,
      });
    }
    for (const outcome of PI_RPC_OUTCOMES) {
      expect(validateAttribute('pi.rpc', 'pi.rpc.outcome', outcome)).toEqual({
        valid: true,
      });
    }
  });

  it('rejects an unreviewed rpc outcome', () => {
    expect(
      validateAttribute('pi.rpc', 'pi.rpc.outcome', 'not-a-real-outcome'),
    ).toEqual({ valid: false, error: 'unknown-value' });
  });

  it('accepts every reviewed process reason and resolution', () => {
    for (const reason of PI_PROCESS_STOP_REASONS) {
      expect(
        validateAttribute(
          'pi.process.lifecycle',
          'tau.process.stop_reason',
          reason,
        ),
      ).toEqual({ valid: true });
    }
    for (const resolution of PI_PROCESS_RESOLUTIONS) {
      expect(
        validateAttribute(
          'pi.process.lifecycle',
          'tau.process.resolution',
          resolution,
        ),
      ).toEqual({ valid: true });
    }
  });

  it('accepts stream aggregate counts', () => {
    expect(validateAttribute('pi.stream', 'pi.stream.delta_count', 3)).toEqual({
      valid: true,
    });
    expect(
      validateAttribute('pi.stream', 'pi.stream.character_count', 42),
    ).toEqual({ valid: true });
  });

  it('never treats a forbidden-content canary as a reviewed categorical value', () => {
    const categoricalAttributes: Array<[string, string]> = [
      ['ui.action', 'tau.action.name'],
      ['tauri.invoke', 'tau.invoke.command'],
      ['pi.rpc', 'pi.rpc.method'],
      ['pi.rpc', 'pi.rpc.outcome'],
      ['pi.process.lifecycle', 'tau.process.stop_reason'],
      ['pi.process.lifecycle', 'tau.process.resolution'],
    ];
    for (const [family, key] of categoricalAttributes) {
      for (const canary of FORBIDDEN_CONTENT_CANARIES.values()) {
        expect(validateAttribute(family, key, canary)).toEqual({
          valid: false,
          error: 'unknown-value',
        });
      }
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

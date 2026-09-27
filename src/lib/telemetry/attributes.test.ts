import { describe, expect, it } from 'vitest';

import {
  allowedAttribute,
  type AttributeSpec,
  CONTEXT_ATTRIBUTES,
  FAMILIES,
  FRONTEND_ERROR_KINDS,
  FRONTEND_ERROR_SOURCES,
  isMetricSafe,
  PI_OWNERSHIP_KINDS,
  PI_OWNERSHIP_OUTCOMES,
  PI_PROCESS_EXIT_OUTCOMES,
  PI_PROCESS_RESOLUTIONS,
  PI_PROCESS_STOP_REASONS,
  PI_RPC_ANOMALY_KINDS,
  PI_RPC_METHODS,
  PI_RPC_OUTCOMES,
  RESOURCE_ATTRIBUTES,
  TAURI_INVOKE_COMMANDS,
  TAURI_INVOKE_OUTCOMES,
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
    for (const spec of CONTEXT_ATTRIBUTES) {
      expect(spec.metricSafe).toBe(false);
    }
  });

  it('resolves reviewed family and shared attributes, but not unknown keys', () => {
    const spec = allowedAttribute('pi.rpc', 'pi.rpc.method');
    expect(spec?.kind).toBe('string');
    const contextSpec = allowedAttribute('ui.action', 'tau.session.id');
    expect(contextSpec?.metricSafe).toBe(false);
    expect(
      allowedAttribute('does.not.exist', 'tau.session.id'),
    ).toBeUndefined();
    expect(allowedAttribute('ui.action', 'tau.does.not.exist')).toBeUndefined();
  });
});

describe('validateAttribute', () => {
  it('accepts well-formed attributes and rejects unknown keys and types', () => {
    expect(validateAttribute('pi.rpc', 'pi.rpc.method', 'get_state')).toEqual({
      valid: true,
    });
    expect(
      validateAttribute('pi.process.lifecycle', 'tau.process.exit_code', 0),
    ).toEqual({ valid: true });
    expect(validateAttribute('pi.rpc', 'pi.rpc.transcript', 'hello')).toEqual({
      valid: false,
      error: 'unknown-attribute',
    });
    expect(
      validateAttribute('tauri.invoke', 'tau.invoke.command', 'user content'),
    ).toEqual({ valid: false, error: 'unknown-value' });
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

  it('bounds numeric and UTF-8 attribute values', () => {
    for (const value of [1.5, Number.MAX_SAFE_INTEGER + 1]) {
      expect(
        validateAttribute(
          'pi.process.lifecycle',
          'tau.process.exit_code',
          value,
        ),
      ).toEqual({ valid: false, error: 'wrong-type' });
    }
    for (const value of [-1, 1_000_000_001]) {
      expect(
        validateAttribute(
          'frontend.heartbeat',
          'tau.heartbeat.pending_rpc_count',
          value,
        ),
      ).toEqual({ valid: false, error: 'out-of-range' });
    }
    expect(validateAttribute('pi.rpc', 'pi.generation', -1)).toEqual({
      valid: false,
      error: 'out-of-range',
    });
    const ascii = 'x'.repeat(DEFAULT_MAX_ATTRIBUTE_LEN + 1);
    const multibyte = 'é'.repeat(DEFAULT_MAX_ATTRIBUTE_LEN / 2 + 1);
    for (const oversized of [ascii, multibyte]) {
      expect(validateAttribute('pi.rpc', 'pi.rpc.method', oversized)).toEqual({
        valid: false,
        error: 'too-long',
      });
    }
  });

  it('allows exactly the reviewed action names', () => {
    for (const name of UI_ACTION_NAMES) {
      expect(validateAttribute('ui.action', 'tau.action.name', name)).toEqual({
        valid: true,
      });
    }
    expect(
      validateAttribute('ui.action', 'tau.action.name', 'not.a.real.action'),
    ).toEqual({ valid: false, error: 'unknown-value' });
  });

  it('accepts every reviewed invoke command and outcome', () => {
    expect(TAURI_INVOKE_COMMANDS).toEqual(
      expect.arrayContaining([
        'update_snapshot',
        'check_for_update',
        'get_dismissed_update_version',
        'set_dismissed_update_version',
        'download_update',
        'request_update_restart',
        'install_update',
      ]),
    );
    for (const command of TAURI_INVOKE_COMMANDS) {
      expect(
        validateAttribute('tauri.invoke', 'tau.invoke.command', command),
      ).toEqual({ valid: true });
    }
    for (const outcome of TAURI_INVOKE_OUTCOMES) {
      expect(
        validateAttribute('tauri.invoke', 'tau.invoke.outcome', outcome),
      ).toEqual({ valid: true });
    }
  });

  it('allows reviewed RPC methods and outcomes but not unreviewed outcomes', () => {
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
    for (const kind of PI_RPC_ANOMALY_KINDS) {
      expect(
        validateAttribute('pi.rpc.anomaly', 'pi.rpc.anomaly.kind', kind),
      ).toEqual({ valid: true });
    }
    expect(
      validateAttribute('pi.rpc', 'pi.rpc.outcome', 'not-a-real-outcome'),
    ).toEqual({ valid: false, error: 'unknown-value' });
  });

  it('accepts only reviewed ownership categories', () => {
    for (const kind of PI_OWNERSHIP_KINDS) {
      expect(
        validateAttribute('pi.ownership', 'tau.ownership.kind', kind),
      ).toEqual({ valid: true });
    }
    for (const outcome of PI_OWNERSHIP_OUTCOMES) {
      expect(
        validateAttribute('pi.ownership', 'tau.ownership.outcome', outcome),
      ).toEqual({ valid: true });
    }
    expect(
      validateAttribute('pi.ownership', 'tau.ownership.stale_process_count', 2),
    ).toEqual({ valid: true });
    expect(
      validateAttribute('pi.ownership', 'tau.ownership.kind', 'owner-secret'),
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
    for (const outcome of PI_PROCESS_EXIT_OUTCOMES) {
      expect(
        validateAttribute(
          'pi.process.lifecycle',
          'tau.process.exit_outcome',
          outcome,
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

  it('allows only reviewed frontend error sources and kinds', () => {
    for (const source of FRONTEND_ERROR_SOURCES) {
      expect(
        validateAttribute('frontend.error', 'tau.error.source', source),
      ).toEqual({ valid: true });
    }
    for (const kind of FRONTEND_ERROR_KINDS) {
      expect(
        validateAttribute('frontend.error', 'tau.error.kind', kind),
      ).toEqual({ valid: true });
    }
    expect(
      validateAttribute('frontend.error', 'tau.error.source', 'not-a-source'),
    ).toEqual({ valid: false, error: 'unknown-value' });
    expect(
      validateAttribute('frontend.error', 'tau.error.kind', 'not-a-kind'),
    ).toEqual({ valid: false, error: 'unknown-value' });
  });

  it('accepts a sanitized panic/error location as a bounded free-form string', () => {
    expect(
      validateAttribute('rust.panic', 'tau.error.location', 'pi.rs:42:5'),
    ).toEqual({ valid: true });
    expect(
      validateAttribute(
        'frontend.error',
        'tau.error.location',
        'index.ts:10:4',
      ),
    ).toEqual({ valid: true });
  });

  it('never treats a forbidden-content canary as a reviewed categorical value', () => {
    const categoricalAttributes: Array<[string, string]> = [
      ['ui.action', 'tau.action.name'],
      ['tauri.invoke', 'tau.invoke.command'],
      ['tauri.invoke', 'tau.invoke.outcome'],
      ['pi.rpc', 'pi.rpc.method'],
      ['pi.rpc', 'pi.rpc.outcome'],
      ['pi.rpc.anomaly', 'pi.rpc.anomaly.kind'],
      ['pi.process.lifecycle', 'tau.process.stop_reason'],
      ['pi.process.lifecycle', 'tau.process.resolution'],
      ['pi.process.lifecycle', 'tau.process.exit_outcome'],
      ['frontend.error', 'tau.error.source'],
      ['frontend.error', 'tau.error.kind'],
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

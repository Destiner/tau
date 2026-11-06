import { describe, expect, it, vi } from 'vitest';

import register from '../src-tauri/pi-extensions/tau-reload';

type API = Parameters<typeof register>[0];
type Command = Parameters<API['registerCommand']>[1];
type Context = Parameters<Command['handler']>[1];

function command(): Command {
  const registerCommand = vi.fn<API['registerCommand']>();
  register({ registerCommand });
  expect(registerCommand).toHaveBeenCalledExactlyOnceWith(
    'tau-internal-reload-v1',
    expect.objectContaining({ handler: expect.any(Function) }),
  );
  return registerCommand.mock.calls[0]![1];
}

describe('Tau-only reload helper', () => {
  it('reloads once and never uses the invalidated runtime afterward', async () => {
    let invalidated = false;
    const reload = vi.fn(async () => {
      invalidated = true;
    });
    const ctx = new Proxy<Context>(
      { isIdle: (): boolean => true, reload },
      {
        get(target, property, receiver): unknown {
          if (invalidated) throw new Error('invalidated runtime accessed');
          return Reflect.get(target, property, receiver);
        },
      },
    );
    await command().handler('  ', ctx);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it('rejects unsupported arguments without accessing context', async () => {
    const ctx = new Proxy<Context>(
      {},
      {
        get(): never {
          throw new Error('context must not be accessed');
        },
      },
    );
    await expect(command().handler('extra', ctx)).rejects.toThrow(
      'does not accept arguments',
    );
  });

  it.each([
    {},
    { isIdle: (): boolean => true },
    { reload: async (): Promise<void> => {} },
  ])('rejects unavailable required APIs: %o', async (ctx) => {
    await expect(command().handler('', ctx)).rejects.toThrow(
      'does not support',
    );
  });

  it('rejects busy Pi instead of waiting or queueing', async () => {
    const reload = vi.fn(async () => {});
    await expect(
      command().handler('', { isIdle: () => false, reload }),
    ).rejects.toThrow('Pi is busy');
    expect(reload).not.toHaveBeenCalled();
  });

  it('propagates a reload failure for Pi extension_error reporting', async () => {
    const reload = vi.fn(async () => {
      throw new Error('reload failed');
    });
    await expect(
      command().handler('', { isIdle: () => true, reload }),
    ).rejects.toThrow('Pi resource reload failed.');
    expect(reload).toHaveBeenCalledTimes(1);
  });
});

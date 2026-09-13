import { createServer } from 'vite';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import runTauri from './tauri-command';

vi.mock('vite', async (importOriginal) => ({
  ...(await importOriginal<typeof import('vite')>()),
  createServer: vi.fn(),
}));

const createServerMock = vi.mocked(createServer);

function fakeServer(): {
  listen: ReturnType<typeof vi.fn>;
  close: ReturnType<typeof vi.fn>;
  printUrls: ReturnType<typeof vi.fn>;
  resolvedUrls: { local: string[]; network: string[] };
} {
  return {
    listen: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    printUrls: vi.fn(),
    resolvedUrls: {
      local: ['http://localhost:15432/'],
      network: [] as string[],
    },
  };
}

function overrideFrom(args: string[]): {
  build: { devUrl: string; beforeDevCommand: null };
} {
  const index = args.lastIndexOf('--config');
  return JSON.parse(args[index + 1]!) as {
    build: { devUrl: string; beforeDevCommand: null };
  };
}

beforeEach(() => {
  vi.resetAllMocks();
});

describe('Tauri launcher', () => {
  it.each([['build'], ['info'], ['--help'], ['dev', '--help'], ['dev', '-V']])(
    'passes %j through without starting Vite',
    async (...args) => {
      const invoke = vi.fn().mockResolvedValue(undefined);
      await runTauri(args, invoke);
      expect(invoke).toHaveBeenCalledWith(args);
      expect(createServerMock).not.toHaveBeenCalled();
    },
  );

  it('passes the listening URL to Tauri and keeps the server alive until it exits', async () => {
    const server = fakeServer();
    createServerMock.mockResolvedValue(
      server as unknown as Awaited<ReturnType<typeof createServer>>,
    );
    await runTauri(['dev'], async (args) => {
      expect(server.listen).toHaveBeenCalledOnce();
      expect(server.close).not.toHaveBeenCalled();
      expect(overrideFrom(args)).toEqual({
        build: { devUrl: 'http://localhost:15432/', beforeDevCommand: null },
      });
    });
    expect(server.close).toHaveBeenCalledOnce();
  });

  it('preserves custom config and runner/app arguments', async () => {
    const server = fakeServer();
    createServerMock.mockResolvedValue(
      server as unknown as Awaited<ReturnType<typeof createServer>>,
    );
    const invoke = vi.fn().mockResolvedValue(undefined);
    await runTauri(
      ['dev', '--config', 'custom.json', '--', '--locked', '--', '--help'],
      invoke,
    );
    const args = invoke.mock.calls[0]![0] as string[];
    expect(args.slice(0, 4)).toEqual([
      'dev',
      '--config',
      'custom.json',
      '--config',
    ]);
    expect(args.slice(5)).toEqual(['--', '--locked', '--', '--help']);
  });

  it('uses a network URL when Vite binds to a non-loopback host', async () => {
    const server = fakeServer();
    server.resolvedUrls = { local: [], network: ['http://192.0.2.1:15432/'] };
    createServerMock.mockResolvedValue(
      server as unknown as Awaited<ReturnType<typeof createServer>>,
    );
    await runTauri(['dev'], async (args) => {
      expect(overrideFrom(args).build.devUrl).toBe('http://192.0.2.1:15432/');
    });
  });

  it.each(['listen', 'invoke', 'url'])(
    'closes Vite on a %s failure',
    async (failure) => {
      const server = fakeServer();
      const invoke = vi.fn().mockResolvedValue(undefined);
      if (failure === 'listen')
        server.listen.mockRejectedValue(new Error('Listen failed'));
      if (failure === 'invoke')
        invoke.mockRejectedValue(new Error('Tauri failed'));
      if (failure === 'url') server.resolvedUrls = { local: [], network: [] };
      createServerMock.mockResolvedValue(
        server as unknown as Awaited<ReturnType<typeof createServer>>,
      );
      await expect(runTauri(['dev'], invoke)).rejects.toThrow();
      expect(server.close).toHaveBeenCalledOnce();
      if (failure !== 'invoke') expect(invoke).not.toHaveBeenCalled();
    },
  );

  it('runs two real Vite servers concurrently with distinct reachable URLs', async () => {
    const vite = await vi.importActual<typeof import('vite')>('vite');
    createServerMock.mockImplementation(() =>
      vite.createServer({
        logLevel: 'silent',
        optimizeDeps: { noDiscovery: true, include: [] },
      }),
    );
    const urls: string[] = [];
    await runTauri(['dev'], async (firstArgs) => {
      const firstUrl = overrideFrom(firstArgs).build.devUrl;
      urls.push(firstUrl);
      await runTauri(['dev'], async (secondArgs) => {
        const secondUrl = overrideFrom(secondArgs).build.devUrl;
        urls.push(secondUrl);
        expect(secondUrl).not.toBe(firstUrl);
        for (const url of urls) {
          const response = await fetch(url);
          expect(response.ok).toBe(true);
          expect(await response.text()).toContain('/src/main.ts');
        }
      });
      expect((await fetch(firstUrl)).ok).toBe(true);
      await expect(fetch(urls[1]!)).rejects.toThrow();
    });
    await expect(fetch(urls[0]!)).rejects.toThrow();
  }, 20_000);
});

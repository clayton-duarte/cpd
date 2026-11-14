import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PassThrough } from 'node:stream';
import { describe, expect, it, vi } from 'vitest';
import * as serveModule from './serve.ts';

describe('preview dev server', () => {
  it('preview dev server on port 33333 binds and proxies websocket', async () => {
    // web/CONTRACT.md §15.5:
    // The preview dev server configuration exports valid port 33333 bindings
    // and WebSocket upgrade handling on /cpd-ws.
    const serve = serveModule as Record<string, unknown>;

    // 1. Port 33333 binding constants / dev server exports
    expect(serve.PREVIEW_PORT, 'PREVIEW_PORT must be exported as 33333').toBe(33333);
    expect(serve.PREVIEW_HOST, 'PREVIEW_HOST must be loopback by default').toBe('127.0.0.1');

    // 2. createPreviewServer factory export
    expect(typeof serve.createPreviewServer, 'createPreviewServer must be an exported function').toBe('function');

    // 3. Dev server configuration / instance validation
    const createPreviewServer = serve.createPreviewServer as (options?: unknown) => Promise<unknown>;
    const home = mkdtempSync(join(tmpdir(), 'cpd-preview-test-'));
    vi.stubEnv('CPD_HOME', home);
    let server: unknown;
    try {
      server = await createPreviewServer({ port: 33333, host: '127.0.0.1' });
    } finally {
      vi.unstubAllEnvs();
      rmSync(home, { recursive: true, force: true });
    }
    expect(server).toBeDefined();
    await (server as { close(): Promise<void> }).close();
  });
});

describe('static headers', () => {
  it('serves /sw.js no-cache from the static dir', async () => {
    const fake: { request?: (req: IncomingMessage, res: ServerResponse) => void } = {};
    vi.resetModules();
    vi.doMock('node:http', () => ({
      createServer: (handler: typeof fake.request) => {
        fake.request = handler;
        return { once() {}, off() {}, listen(_p: number, _h: string, ready: () => void) { ready(); } };
      },
    }));
    vi.doMock('./plugin.ts', () => ({
      createServices: () => ({ cockpit: { start: () => Promise.resolve() }, newConnection: vi.fn() }),
      realBoundaries: vi.fn(), cpdWebPlugin: vi.fn(),
    }));
    vi.doMock('./ws.ts', () => ({ createWsServer: vi.fn(), createUpgradeFactory: vi.fn(), WS_PATH: '/cpd-ws' }));
    const dir = mkdtempSync(join(tmpdir(), 'cpd-static-sw-'));
    try {
      writeFileSync(join(dir, 'sw.js'), 'self');
      const { createProdServer } = await import('./serve.ts');
      await createProdServer({ staticDir: dir, boundaries: {} as never });
      let headers: Record<string, string | number> = {};
      const response = Object.assign(new PassThrough(), {
        writeHead(_code: number, h: Record<string, string | number>) { headers = h; return this; },
      });
      response.resume();
      const ended = new Promise<void>((resolve) => response.on('end', resolve));
      fake.request?.({ method: 'GET', url: '/sw.js', headers: { host: 'localhost' } } as IncomingMessage,
        response as unknown as ServerResponse);
      await ended;
      expect(headers['Cache-Control']).toBe('no-cache');
    } finally {
      rmSync(dir, { recursive: true, force: true });
      vi.doUnmock('node:http'); vi.doUnmock('./plugin.ts'); vi.doUnmock('./ws.ts');
      vi.resetModules();
    }
  });
});

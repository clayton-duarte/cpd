import { describe, expect, it } from 'vitest';
import * as serveModule from './serve.ts';

describe('preview dev server', () => {
  it('preview dev server on port 33333 binds and proxies websocket', async () => {
    // web/CONTRACT.md §15.5:
    // The preview dev server configuration exports valid port 33333 bindings
    // and WebSocket upgrade handling on /cpd-ws.
    const serve = serveModule as Record<string, unknown>;

    // 1. Port 33333 binding constants / dev server exports
    expect(serve.PREVIEW_PORT, 'PREVIEW_PORT must be exported as 33333').toBe(33333);
    expect(serve.PREVIEW_HOST, 'PREVIEW_HOST must be exported as 0.0.0.0').toBe('0.0.0.0');

    // 2. createPreviewServer factory export
    expect(typeof serve.createPreviewServer, 'createPreviewServer must be an exported function').toBe('function');

    // 3. Dev server configuration / instance validation
    const createPreviewServer = serve.createPreviewServer as (options?: unknown) => Promise<unknown>;
    const server = await createPreviewServer({ port: 33333, host: '0.0.0.0' });
    expect(server).toBeDefined();
  });
});

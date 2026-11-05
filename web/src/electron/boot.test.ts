// @vitest-environment node
import { describe, expect, it, vi } from 'vitest';
import { boot, type BootDeps, type BootWindow } from './boot';

function setup(opts: { env?: Record<string, string>; state?: string; destroyed?: boolean; deferReady?: boolean; reloadServer?: () => Promise<void>; loadDevUrl?: BootDeps['loadDevUrl'] } = {}) {
  const log: string[] = [];
  const handlers: Record<string, () => void> = {};
  const titles: unknown[] = [];
  let reloads = 0;
  let destroyed = opts.destroyed ?? false;
  let release: () => void = () => {};
  const ready = opts.deferReady ? new Promise<void>((r) => (release = r)) : Promise.resolve();
  class FakeWindow implements BootWindow {
    webContents = { isDestroyed: () => destroyed, reloadIgnoringCache: () => (log.push('reloadRenderer'), void (reloads += 1)) };
    constructor(options: { title: string }) {
      titles.push(options.title);
      log.push('window');
    }
  }
  const deps: BootDeps = {
    app: { setName: (n) => void log.push(`setName:${n}`), whenReady: () => (log.push('whenReady'), ready) },
    BrowserWindow: FakeWindow,
    powerMonitor: {},
    process: { on: (s, h) => void (handlers[s] = h) },
    startAutoUpdate: async () => (log.push('startAutoUpdate'), { bootstrap: { state: opts.state ?? 'ready' } }),
    startServices: () => void log.push('startServices'),
    registerIpc: () => void log.push('registerIpc'),
    registerWebuiIpc: () => void log.push('registerWebuiIpc'),
    loadDevUrl: opts.loadDevUrl ?? (async (_w, url) => void log.push(`loadDevUrl:${url}`)),
    loadFile: async (_w, p) => void log.push(`loadFile:${p}`),
    env: opts.env ?? {},
    reloadServer: opts.reloadServer ?? (async () => void log.push('reloadServer')),
  };
  return { deps, log, handlers, titles, release, reloads: () => reloads, setDestroyed: (v: boolean) => (destroyed = v) };
}

describe('boot', () => {
  it('boot names the app CPD before the app is ready', async () => {
    const s = setup({ deferReady: true });
    const p = boot(s.deps).catch(() => undefined);
    expect(s.log.indexOf('setName:CPD')).toBeGreaterThanOrEqual(0);
    s.release();
    await p;
    expect(s.log.indexOf('setName:CPD')).toBeLessThan(s.log.indexOf('registerIpc'));
  });
  it('boot creates the window titled CPD', async () => {
    const s = setup();
    await boot(s.deps);
    expect(s.titles).toEqual(['CPD']);
  });
  it('boot registers IPC and starts the update before loading the renderer', async () => {
    const s = setup();
    await boot(s.deps);
    const order = s.log.filter((l) => !l.startsWith('setName') && l !== 'whenReady' && l !== 'window');
    expect(order).toHaveLength(5);
    expect(order.slice(0, 4)).toEqual(['registerIpc', 'registerWebuiIpc', 'startAutoUpdate', 'startServices']);
    expect(order[4]).toMatch(/^loadFile:/);
  });
  it('boot starts services only when the bootstrap is ready', async () => {
    const s = setup({ state: 'failed' });
    await boot(s.deps);
    expect(s.log).toContain('startAutoUpdate');
    expect(s.log).not.toContain('startServices');
  });
  it('boot loads the dev URL when CPD_ELECTRON_DEV_URL is set', async () => {
    const s = setup({ env: { CPD_ELECTRON_DEV_URL: 'http://localhost:5173/electron.html' } });
    await boot(s.deps);
    expect(s.log).toContain('loadDevUrl:http://localhost:5173/electron.html');
    expect(s.log.some((l) => l.startsWith('loadFile:'))).toBe(false);
  });
  it('boot loads the built renderer when the dev URL fails to load', async () => {
    const url = 'http://localhost:5173/electron.html';
    const s = setup({ env: { CPD_ELECTRON_DEV_URL: url }, loadDevUrl: async () => Promise.reject(new Error('ERR_CONNECTION_REFUSED')) });
    const spy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    try {
      await expect(boot(s.deps)).resolves.toBeUndefined();
    } finally {
      spy.mockRestore();
    }
    expect(s.log.filter((l) => l.startsWith('loadFile:'))).toEqual(['loadFile:renderer/electron.html']);
  });
  it('boot logs the dev URL failure before loading the built renderer', async () => {
    const url = 'http://localhost:5173/electron.html';
    const s = setup({ env: { CPD_ELECTRON_DEV_URL: url }, loadDevUrl: async () => Promise.reject(new Error('ERR_CONNECTION_REFUSED')) });
    const spy = vi.spyOn(console, 'error').mockImplementation((...a) => void s.log.push(`error:${a.map(String).join(' ')}`));
    try {
      await boot(s.deps).catch(() => undefined);
    } finally {
      spy.mockRestore();
    }
    const at = s.log.findIndex((l) => l.startsWith('error:') && l.includes(url));
    expect(at).toBeGreaterThanOrEqual(0);
    expect(at).toBeLessThan(s.log.findIndex((l) => l.startsWith('loadFile:')));
  });
  it('boot registers a SIGUSR1 reload that skips a destroyed webContents', async () => {
    const s = setup();
    await boot(s.deps);
    const handler = s.handlers['SIGUSR1'];
    expect(handler).toBeTypeOf('function');
    await Promise.resolve(handler?.());
    expect(s.reloads()).toBe(1);
    s.setDestroyed(true);
    await Promise.resolve(handler?.());
    expect(s.reloads()).toBe(1);
  });
  it('SIGUSR1 reloads the server before the renderer', async () => {
    const s = setup();
    await boot(s.deps);
    await Promise.resolve(s.handlers['SIGUSR1']?.());
    const tail = s.log.filter((l) => l === 'reloadServer' || l === 'reloadRenderer');
    expect(tail).toEqual(['reloadServer', 'reloadRenderer']);
  });
  it('a failing server reload is logged and the renderer still reloads', async () => {
    const s = setup({ reloadServer: async () => Promise.reject(new Error('boom')) });
    const errors: unknown[][] = [];
    const spy = vi.spyOn(console, 'error').mockImplementation((...a) => void errors.push(a));
    await boot(s.deps);
    await expect(Promise.resolve(s.handlers['SIGUSR1']?.())).resolves.toBeUndefined();
    spy.mockRestore();
    expect(errors.length).toBeGreaterThan(0);
    expect(s.reloads()).toBe(1);
  });
  // Assumption: the server reload is independent of the window, so it runs even when the window is destroyed.
  it('SIGUSR1 does nothing when the window is destroyed', async () => {
    const s = setup({ destroyed: true });
    await boot(s.deps);
    await Promise.resolve(s.handlers['SIGUSR1']?.());
    expect(s.reloads()).toBe(0);
    expect(s.log).toContain('reloadServer');
    expect(s.log).not.toContain('reloadRenderer');
  });
});

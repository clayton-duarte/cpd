import { describe, expect, it, vi } from 'vitest';
import { swapBundle } from './bundle';
import type { SyncResult } from './checkout';
import { createUpdater, type UpdaterDeps } from './controller';
import { fakeExec, fakeFs } from './fakes';
import { rebuildViews } from './preview';
import type { UpdateExec, UpdateState } from './types';
import { BUILT_SHA_FILE, PREVIEW_VIEW_DIR, PROD_VIEW_DIR } from './views';

const OK = { ok: true } as const;
const SHA = 'abc123';

// D1: an update that changes a non-renderer build artefact relaunches the main process through an
// injected port; a renderer-only change keeps the in-place renderer reload.
type RelaunchingDeps = UpdaterDeps & { relaunch(): void };
const ROOT = '/repo';
const bundle = (files: Record<string, string>) => fakeFs(files);
const withRelaunch = (deps: UpdaterDeps, relaunch: () => void): RelaunchingDeps => {
  (deps as unknown as { relaunch(): void }).relaunch = relaunch;
  return deps as RelaunchingDeps;
};

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => { resolve = r; });
  return { promise, resolve };
}

// The views-impl change adds the view rebuild to the updater's dependencies; pin it here so the
// controller test compiles today and the implementing change can wire it in.
type UpdaterDepsWithViews = UpdaterDeps & { rebuildViews(): Promise<void> };

function setup(over: Partial<UpdaterDepsWithViews> = {}) {
  const events: string[] = [];
  const states: UpdateState[] = [];
  const logs: string[] = [];
  const deps: UpdaterDepsWithViews = {
    sync: vi.fn(async (): Promise<SyncResult> => ({ kind: 'updated', from: 'old', to: SHA, lockfileChanged: false })),
    headSha: vi.fn(async () => SHA),
    builtSha: vi.fn(() => 'old'),
    cliSha: vi.fn(() => SHA),
    build: vi.fn(async () => { events.push('build'); return OK; }),
    swap: vi.fn(() => { events.push('swap'); return { mainChanged: false }; }),
    whenApplicable: vi.fn(async () => { events.push('gate'); }),
    reloadRenderer: vi.fn(() => { events.push('reload'); }),
    installCli: vi.fn(async () => OK),
    rebuildViews: vi.fn(async () => {}),
    onState: vi.fn((s: UpdateState) => { states.push(s); events.push(`state:${s.phase}`); }),
    log: vi.fn((l: string) => { logs.push(l); }),
    ...over,
  };
  return { deps, events, states, logs, updater: createUpdater(deps) };
}

const phases = (states: UpdateState[]) => states.map((s) => s.phase);

describe('createUpdater', () => {
  it('check does nothing when sync is skipped', async () => {
    const { deps, states, logs, updater } = setup({ sync: vi.fn(async (): Promise<SyncResult> => ({ kind: 'skipped', reason: 'dirty' })) });
    await updater.check();
    expect(phases(states)).toEqual(['syncing', 'idle']);
    expect(logs.join('\n')).toContain('dirty');
    expect(deps.build).not.toHaveBeenCalled();
    expect(deps.installCli).not.toHaveBeenCalled();
  });

  it('check builds swaps and reloads when the checkout advanced', async () => {
    const { deps, events, states, updater } = setup();
    await updater.check();
    expect(deps.build).toHaveBeenCalledWith({ lockfileChanged: false, sha: SHA });
    expect(phases(states)).toEqual(['syncing', 'building', 'waiting', 'idle']);
    expect(events.filter((e) => ['build', 'gate', 'swap', 'reload'].includes(e))).toEqual(['build', 'gate', 'swap', 'reload']);
  });

  it('check rebuilds when the stamp differs from HEAD', async () => {
    const { deps, updater } = setup({ sync: vi.fn(async (): Promise<SyncResult> => ({ kind: 'current', sha: SHA })), builtSha: vi.fn(() => 'stale') });
    await updater.check();
    expect(deps.build).toHaveBeenCalledTimes(1);
  });

  it('check does not build when the stamp equals HEAD', async () => {
    const { deps, states, updater } = setup({ sync: vi.fn(async (): Promise<SyncResult> => ({ kind: 'current', sha: SHA })), builtSha: vi.fn(() => SHA) });
    await updater.check();
    expect(deps.build).not.toHaveBeenCalled();
    expect(deps.swap).not.toHaveBeenCalled();
    expect(deps.reloadRenderer).not.toHaveBeenCalled();
    expect(states[states.length - 1]).toEqual({ phase: 'idle' });
  });

  it('check passes lockfileChanged from the sync result', async () => {
    const updated = setup({ sync: vi.fn(async (): Promise<SyncResult> => ({ kind: 'updated', from: 'o', to: SHA, lockfileChanged: true })) });
    await updated.updater.check();
    expect(updated.deps.build).toHaveBeenCalledWith({ lockfileChanged: true, sha: SHA });
    const current = setup({ sync: vi.fn(async (): Promise<SyncResult> => ({ kind: 'current', sha: SHA })), builtSha: vi.fn(() => 'stale') });
    await current.updater.check();
    expect(current.deps.build).toHaveBeenCalledWith({ lockfileChanged: true, sha: SHA });
  });

  it('check waits for the apply gate before swapping', async () => {
    const gate = deferred<void>();
    const { deps, states, updater } = setup({ whenApplicable: vi.fn(() => gate.promise) });
    const run = updater.check();
    await vi.waitFor(() => expect(deps.whenApplicable).toHaveBeenCalled());
    expect(phases(states)).toContain('waiting');
    expect(deps.swap).not.toHaveBeenCalled();
    expect(deps.reloadRenderer).not.toHaveBeenCalled();
    gate.resolve();
    await run;
    expect(deps.swap).toHaveBeenCalledTimes(1);
    expect(deps.reloadRenderer).toHaveBeenCalledTimes(1);
  });

  it('check does not reload when main.js changed and reports restart-needed', async () => {
    const { deps, states, updater } = setup({ swap: vi.fn(() => ({ mainChanged: true })) });
    await updater.check();
    expect(deps.reloadRenderer).not.toHaveBeenCalled();
    expect(states[states.length - 1]).toEqual({ phase: 'restart-needed' });
    expect(phases(states)).not.toContain('idle');
  });

  it('check keeps restart-needed sticky and never reloads afterwards', async () => {
    const mainChanged = [true, false];
    const { deps, states, updater } = setup({ swap: vi.fn(() => ({ mainChanged: mainChanged.shift() ?? false })) });
    await updater.check();
    await updater.check();
    expect(deps.swap).toHaveBeenCalledTimes(2);
    expect(deps.reloadRenderer).not.toHaveBeenCalled();
    expect(states[states.length - 1]).toEqual({ phase: 'restart-needed' });
  });

  it('check reports error and keeps the old bundle when the build fails', async () => {
    const { deps, states, updater } = setup({ build: vi.fn(async () => ({ ok: false as const, log: 'compiling\nboom: it broke\n' })) });
    await updater.check();
    expect(states[states.length - 1]).toEqual({ phase: 'error', message: 'boom: it broke' });
    expect(deps.swap).not.toHaveBeenCalled();
    expect(deps.reloadRenderer).not.toHaveBeenCalled();
  });

  it('check installs the CLI when its stamp differs from HEAD', async () => {
    const { deps, updater } = setup({ cliSha: vi.fn(() => 'older') });
    await updater.check();
    expect(deps.installCli).toHaveBeenCalledWith(SHA);
  });

  it('check skips the CLI install when its stamp equals HEAD', async () => {
    const { deps, updater } = setup({ cliSha: vi.fn(() => SHA) });
    await updater.check();
    expect(deps.installCli).not.toHaveBeenCalled();
  });

  it('rebuilds the views when the CLI update lands', async () => {
    const PREVIEW = '/preview';
    const BUILD = 'pnpm --filter cpd-web-client run build:electron';
    const outs: (string | undefined)[] = [];
    const fx = fakeExec({
      'git rev-parse HEAD': { stdout: `${SHA}\n` },
      'git rev-parse --abbrev-ref HEAD': { stdout: 'preview\n' },
      [BUILD]: {},
    });
    const fs = fakeFs({ [`${ROOT}/.git/HEAD`]: '', [`${PREVIEW}/.git/HEAD`]: '' });
    const exec: UpdateExec = async (cmd, args, opts) => {
      const r = await fx.exec(cmd, args, opts);
      if (cmd === 'pnpm') outs.push(opts.env?.['CPD_ELECTRON_OUT']);
      return r;
    };
    const rebuilt = vi.fn(async () => {
      await rebuildViews({ exec, fs }, { prod: ROOT, preview: PREVIEW });
    });
    const { deps, updater } = setup({
      cliSha: vi.fn(() => 'older'),
      installCli: vi.fn(async () => OK),
      rebuildViews: rebuilt,
    });

    await updater.check();

    expect(deps.installCli).toHaveBeenCalledWith(SHA);
    expect(rebuilt).toHaveBeenCalledTimes(1);
    expect(outs).toEqual([PROD_VIEW_DIR, PREVIEW_VIEW_DIR]);
    expect(fs.readText(`${ROOT}/${PROD_VIEW_DIR}/${BUILT_SHA_FILE}`)).toBe(SHA);
    expect(fs.readText(`${PREVIEW}/${PREVIEW_VIEW_DIR}/${BUILT_SHA_FILE}`)).toBe(SHA);
  });

  it('does not rebuild the views when the CLI is already current', async () => {
    const { deps, updater } = setup({ cliSha: vi.fn(() => SHA) });
    await updater.check();
    expect(deps.rebuildViews).not.toHaveBeenCalled();
  });

  it('check logs a CLI install failure without failing the bundle flow', async () => {
    const { deps, states, logs, updater } = setup({ cliSha: vi.fn(() => null), installCli: vi.fn(async () => ({ ok: false as const, log: 'cargo exploded' })) });
    await updater.check();
    expect(logs.join('\n')).toContain('cargo exploded');
    expect(phases(states)).not.toContain('error');
    expect(deps.reloadRenderer).toHaveBeenCalledTimes(1);
    expect(states[states.length - 1]).toEqual({ phase: 'idle' });
  });

  it('check coalesces overlapping calls into one trailing run', async () => {
    const first = deferred<SyncResult>();
    const results = [first.promise];
    const sync = vi.fn(() => results.shift() ?? Promise.resolve<SyncResult>({ kind: 'current', sha: SHA }));
    const { updater } = setup({ sync, builtSha: vi.fn(() => SHA) });
    const a = updater.check();
    const b = updater.check();
    const c = updater.check();
    expect(sync).toHaveBeenCalledTimes(1);
    first.resolve({ kind: 'current', sha: SHA });
    await Promise.all([a, b, c]);
    await updater.check();
    expect(sync).toHaveBeenCalledTimes(3);
  });

  it('relaunches the main process when a non-renderer artefact changed', async () => {
    const fs = bundle({
      [`${ROOT}/web/dist-electron/main.js`]: 'main',
      [`${ROOT}/web/dist-electron/preload.cjs`]: 'preload',
      [`${ROOT}/web/dist-electron/server.js`]: 'old server',
      [`${ROOT}/web/dist-electron/renderer/electron.html`]: 'page',
      [`${ROOT}/web/dist-electron.next/main.js`]: 'main',
      [`${ROOT}/web/dist-electron.next/preload.cjs`]: 'preload',
      [`${ROOT}/web/dist-electron.next/server.js`]: 'new server',
      [`${ROOT}/web/dist-electron.next/renderer/electron.html`]: 'page',
    });
    const { deps, updater } = setup({ swap: vi.fn(() => swapBundle(fs, ROOT)) });
    const relaunch = vi.fn();
    withRelaunch(deps, relaunch);
    await updater.check();
    expect(relaunch).toHaveBeenCalledTimes(1);
    expect(deps.reloadRenderer).not.toHaveBeenCalled();
  });

  it('reloads the renderer in place when only renderer files changed', async () => {
    const fs = bundle({
      [`${ROOT}/web/dist-electron/main.js`]: 'main',
      [`${ROOT}/web/dist-electron/preload.cjs`]: 'preload',
      [`${ROOT}/web/dist-electron/server.js`]: 'server',
      [`${ROOT}/web/dist-electron/renderer/electron.html`]: 'old page',
      [`${ROOT}/web/dist-electron.next/main.js`]: 'main',
      [`${ROOT}/web/dist-electron.next/preload.cjs`]: 'preload',
      [`${ROOT}/web/dist-electron.next/server.js`]: 'server',
      [`${ROOT}/web/dist-electron.next/renderer/electron.html`]: 'new page',
    });
    const { deps, updater } = setup({ swap: vi.fn(() => swapBundle(fs, ROOT)) });
    const relaunch = vi.fn();
    withRelaunch(deps, relaunch);
    await updater.check();
    expect(deps.reloadRenderer).toHaveBeenCalledTimes(1);
    expect(relaunch).not.toHaveBeenCalled();
  });
});

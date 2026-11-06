import { expect, it } from 'vitest';
import { fakeExec, fakeFs, fakeTimers } from './fakes';
import type { FollowProcess } from './follow';
import { startAutoUpdate, type AutoUpdateDeps } from './start';
import type { BootstrapStatus, UpdateState } from './types';

const ROOT = '/repo';
const SHA = 'abc123';
const BASE = ['events', 'follow', '--kind', 'pr-merged'];
const CURRENT = {
  'git rev-parse --abbrev-ref HEAD': { stdout: 'main\n' },
  'git status --porcelain': { stdout: '' },
  'git fetch origin': {},
  'git rev-parse HEAD': { stdout: `${SHA}\n` },
  'git rev-parse origin/main': { stdout: `${SHA}\n` },
};
const UP_TO_DATE_FILES = {
  [`${ROOT}/scripts/install-cpd.sh`]: '',
  [`${ROOT}/.git/HEAD`]: '',
  [`${ROOT}/web/dist-electron/.built-sha`]: SHA,
  [`${ROOT}/target/.cli-installed-sha`]: SHA,
  '/home/u/.local/bin/cpd': '',
};

async function flush() {
  for (let i = 0; i < 50; i++) await Promise.resolve();
}

function setup(opts: { table?: Record<string, object>; files?: Record<string, string>; env?: Record<string, string>; platform?: string } = {}) {
  const fx = fakeExec(opts.table ?? CURRENT);
  const fs = fakeFs(opts.files ?? UP_TO_DATE_FILES);
  const timers = fakeTimers();
  const spawned: { args: string[]; killed: boolean; line(l: string): void; exit(c: number | null): void }[] = [];
  const logs: string[] = [];
  const states: UpdateState[] = [];
  const bootstraps: BootstrapStatus[] = [];
  const deps: AutoUpdateDeps = {
    env: opts.env ?? { CPD_CHECKOUT: ROOT },
    bundleDir: '/nowhere/web/dist-electron',
    home: '/home/u',
    platform: opts.platform ?? 'linux',
    exec: fx.exec,
    fs,
    timers,
    spawnFollow: (args): FollowProcess => {
      let lineCb: (l: string) => void = () => {};
      let exitCb: (c: number | null) => void = () => {};
      const rec = { args, killed: false, line: (l: string) => lineCb(l), exit: (c: number | null) => exitCb(c) };
      spawned.push(rec);
      return { onLine: (cb) => void (lineCb = cb), onExit: (cb) => void (exitCb = cb), kill: () => void (rec.killed = true) };
    },
    isFocused: () => false,
    systemIdleSeconds: () => 0,
    reloadRenderer: () => {},
    onState: (s) => void states.push(s),
    onBootstrap: (s) => void bootstraps.push(s),
    log: (l) => void logs.push(l),
  };
  const fetches = () => fx.calls.filter((c) => c.cmd === 'git' && c.args[0] === 'fetch').length;
  return { deps, fx, timers, spawned, logs, states, bootstraps, fetches };
}

it('startAutoUpdate returns bootstrap ready and starts following', async () => {
  const t = setup();
  const r = await startAutoUpdate(t.deps);
  await flush();
  expect(r.bootstrap).toEqual({ state: 'ready' });
  expect(t.bootstraps).toEqual([{ state: 'ready' }]);
  expect(t.spawned.map((s) => s.args)).toEqual([BASE]);
  expect(t.fetches()).toBe(1);
});

it('startAutoUpdate does not follow when the bootstrap failed', async () => {
  const t = setup({ files: {}, platform: 'darwin', env: {} });
  const r = await startAutoUpdate(t.deps);
  expect(r.bootstrap.state).toBe('failed');
  expect(t.bootstraps.at(-1)?.state).toBe('failed');
  expect(t.spawned).toEqual([]);
  expect(t.fx.calls).toEqual([]);
});

it('startAutoUpdate stays off when no checkout is found but the cli exists', async () => {
  const t = setup({ files: { '/home/u/.local/bin/cpd': '' }, platform: 'darwin', env: {} });
  const r = await startAutoUpdate(t.deps);
  expect(r.bootstrap).toEqual({ state: 'ready' });
  expect(t.logs).toHaveLength(1);
  expect(t.spawned).toEqual([]);
  expect(t.fx.calls).toEqual([]);
  expect(() => r.stop()).not.toThrow();
});

it('startAutoUpdate runs a check for each pr-merged line', async () => {
  const t = setup();
  await startAutoUpdate(t.deps);
  await flush();
  expect(t.fetches()).toBe(1);
  t.spawned[0]?.line(JSON.stringify({ kind: 'pr-merged', id: 7 }));
  await flush();
  expect(t.fetches()).toBe(2);
  t.spawned[0]?.line(JSON.stringify({ kind: 'pr-merged', id: 8 }));
  await flush();
  expect(t.fetches()).toBe(3);
});

it('startAutoUpdate logs a failed check instead of throwing', async () => {
  const t = setup({ table: {} });
  const r = await startAutoUpdate(t.deps);
  await flush();
  expect(r.bootstrap).toEqual({ state: 'ready' });
  expect(t.logs.some((l) => l.includes('unexpected exec'))).toBe(true);
});

it('startAutoUpdate stop stops following', async () => {
  const t = setup();
  const r = await startAutoUpdate(t.deps);
  await flush();
  r.stop();
  expect(t.spawned[0]?.killed).toBe(true);
  t.spawned[0]?.exit(0);
  expect(t.timers.pending).toEqual([]);
  expect(t.spawned).toHaveLength(1);
});

it('startAutoUpdate enables the rebase in preview mode only', async () => {
  const DIVERGED = { ...CURRENT, 'git rev-parse origin/main': { stdout: 'def456\n' }, 'git merge-base --is-ancestor HEAD origin/main': { code: 1 }, 'git diff --name-only HEAD origin/main': {}, 'git status --porcelain --untracked-files=all': {}, 'git rebase --autostash origin/main': {}, 'git diff --name-only abc123 def456': {} };
  const REBASE = 'git rebase --autostash origin/main';
  const rebased = async (mode: string | undefined) => {
    const env: Record<string, string> = { CPD_CHECKOUT: ROOT, ...(mode === undefined ? {} : { CPD_MODE: mode }) };
    const t = setup({ table: DIVERGED, env });
    await startAutoUpdate(t.deps);
    await flush();
    return t.fx.calls.some((c) => `${c.cmd} ${c.args.join(' ')}` === REBASE);
  };
  expect(await rebased('preview')).toBe(true);
  expect(await rebased('production')).toBe(false);
  expect(await rebased(undefined)).toBe(false);
});

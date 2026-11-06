import { describe, expect, it } from 'vitest';
import { resolveCheckout, syncCheckout } from './checkout';
import { fakeExec, fakeFs } from './fakes';

const ROOT = '/repo';
const fsOf = (root: string, ...skip: string[]) =>
  fakeFs(Object.fromEntries([`${root}/scripts/install-cpd.sh`, `${root}/.git/HEAD`].filter((p) => !skip.some((s) => p.includes(s))).map((p) => [p, ''])));

describe('resolveCheckout', () => {
  it('resolveCheckout prefers CPD_CHECKOUT', () => {
    expect(resolveCheckout({ env: { CPD_CHECKOUT: '/env/root' }, bundleDir: '/repo/web/dist-electron', fs: fsOf('/env/root') })).toBe('/env/root');
  });
  it('resolveCheckout falls back to two levels above the bundle dir', () => {
    expect(resolveCheckout({ env: {}, bundleDir: '/repo/web/dist-electron', fs: fsOf('/repo') })).toBe('/repo');
  });
  it('resolveCheckout returns null without install-cpd.sh', () => {
    expect(resolveCheckout({ env: {}, bundleDir: '/repo/web/dist-electron', fs: fsOf('/repo', 'install-cpd.sh') })).toBeNull();
  });
  it('resolveCheckout returns null without a git dir', () => {
    expect(resolveCheckout({ env: {}, bundleDir: '/repo/web/dist-electron', fs: fsOf('/repo', '.git') })).toBeNull();
  });
});

const FETCH = 'git fetch origin';
const HEAD = 'git rev-parse HEAD';
const ORIGIN = 'git rev-parse origin/main';
const ANC = 'git merge-base --is-ancestor HEAD origin/main';
const FF = 'git merge --ff-only origin/main';
const DIFF = 'git diff --name-only aaa bbb';
const CHANGED = 'git diff --name-only HEAD origin/main';
const CHECKOUT = 'git checkout HEAD -- web/a.ts pnpm-lock.yaml';
const STATUS = 'git status --porcelain --untracked-files=all';
const REBASE = 'git rebase --autostash origin/main';
const ABORT = 'git rebase --abort';
const OFF = { rebase: false } as const;
const ON = { rebase: true } as const;
const main = { [FETCH]: {} };

describe('syncCheckout', () => {
  const base = { ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [CHANGED]: {}, [STATUS]: {} };
  const names = (f: ReturnType<typeof fakeExec>) => f.calls.map((c) => `${c.cmd} ${c.args.join(' ')}`);
  it('syncCheckout checks out the files changed in HEAD..origin/main before fast-forwarding', async () => {
    const f = fakeExec({ ...base, [CHANGED]: { stdout: 'web/a.ts\npnpm-lock.yaml\n' }, [STATUS]: { stdout: ' M web/a.ts\n M pnpm-lock.yaml\n' }, [CHECKOUT]: {}, [FF]: {}, [DIFF]: { stdout: 'pnpm-lock.yaml\n' } });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: true });
    const n = names(f);
    expect(n.indexOf(CHANGED)).toBeGreaterThanOrEqual(0);
    expect(n.indexOf(CHECKOUT)).toBeGreaterThan(n.indexOf(CHANGED));
    expect(n.indexOf(FF)).toBeGreaterThan(n.indexOf(CHECKOUT));
  });
  it('syncCheckout removes untracked files that origin/main adds, then fast-forwards', async () => {
    const CLEAN = 'git clean -f -- web/new.ts';
    const f = fakeExec({ ...base, [CHANGED]: { stdout: 'web/new.ts\n' }, [STATUS]: { stdout: '?? web/new.ts\n?? web/mine.ts\n' }, [CLEAN]: {}, [FF]: {}, [DIFF]: { stdout: 'web/new.ts\n' } });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: false });
    const n = names(f);
    expect(n).toContain(STATUS);
    expect(n.indexOf(CLEAN)).toBeGreaterThan(n.indexOf(STATUS));
    expect(n.indexOf(FF)).toBeGreaterThan(n.indexOf(CLEAN));
    expect(n.some((c) => c.startsWith('git checkout'))).toBe(false);
  });
  it('syncCheckout restores only the dirty tracked files that origin/main changes and keeps other edits', async () => {
    const CO = 'git checkout HEAD -- web/a.ts';
    const f = fakeExec({ ...base, [CHANGED]: { stdout: 'web/a.ts\nweb/b.ts\n' }, [STATUS]: { stdout: ' M web/a.ts\n M web/mine.ts\n' }, [CO]: {}, [FF]: {}, [DIFF]: {} });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toMatchObject({ kind: 'updated' });
    const n = names(f);
    expect(n.indexOf(CO)).toBeGreaterThan(n.indexOf(STATUS));
    expect(n.indexOf(FF)).toBeGreaterThan(n.indexOf(CO));
    expect(n.some((c) => c.startsWith('git clean') || c.includes('stash') || c.includes('reset'))).toBe(false);
  });
  it('syncCheckout fast-forwards without cleaning when nothing superseded is dirty', async () => {
    const f = fakeExec({ ...base, [CHANGED]: { stdout: 'web/a.ts\n' }, [STATUS]: { stdout: ' M web/mine.ts\n?? web/x.ts\n' }, [FF]: {}, [DIFF]: {} });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toMatchObject({ kind: 'updated' });
    const n = names(f);
    expect(n).toContain(STATUS);
    expect(n.some((c) => c.startsWith('git checkout') || c.startsWith('git clean'))).toBe(false);
  });
  it('syncCheckout rebases local commits onto origin/main with autostash in preview mode', async () => {
    const f = fakeExec({ ...base, [ANC]: { code: 1 }, [REBASE]: {}, [DIFF]: { stdout: 'pnpm-lock.yaml\n' } });
    expect(await syncCheckout(f.exec, ROOT, ON)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: true });
    const n = names(f);
    expect(n).toContain(REBASE);
    expect(n).not.toContain(FF);
    expect(n).not.toContain(ABORT);
  });
  it('syncCheckout skips with rebase-conflict and aborts the rebase on a conflict', async () => {
    const f = fakeExec({ ...base, [ANC]: { code: 1 }, [REBASE]: { code: 1 }, [ABORT]: {} });
    expect(await syncCheckout(f.exec, ROOT, ON)).toEqual({ kind: 'skipped', reason: 'rebase-conflict' });
    const n = names(f);
    expect(n.indexOf(ABORT)).toBeGreaterThan(n.indexOf(REBASE));
  });
  it('syncCheckout skips diverged checkouts when rebase is off', async () => {
    const f = fakeExec({ ...base, [ANC]: { code: 1 } });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toEqual({ kind: 'skipped', reason: 'diverged' });
    const n = names(f);
    expect(n.indexOf(ANC)).toBeGreaterThan(n.indexOf(STATUS));
    expect(n).toContain(STATUS);
    expect(n.some((c) => c.startsWith('git rebase') || c === FF)).toBe(false);
  });
  it('syncCheckout fast-forwards from any branch that is an ancestor of origin main', async () => {
    const f = fakeExec({ ...base, [CHANGED]: {}, [STATUS]: {}, [FF]: {}, [DIFF]: { stdout: 'web/a.ts\n' } });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toMatchObject({ kind: 'updated' });
    expect(names(f).some((c) => c.includes('--abbrev-ref'))).toBe(false);
  });
  it('syncCheckout skips as diverged when HEAD is not an ancestor', async () => {
    const f = fakeExec({ ...base, [ANC]: { code: 1 }, [CHANGED]: {}, [STATUS]: {} });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toEqual({ kind: 'skipped', reason: 'diverged' });
    expect(names(f).some((c) => c.startsWith('git checkout') || c === FF)).toBe(false);
  });
  it('syncCheckout skips as diverged when the checkout or the merge fails', async () => {
    const a = fakeExec({ ...base, [CHANGED]: { stdout: 'web/a.ts\npnpm-lock.yaml\n' }, [STATUS]: { stdout: ' M web/a.ts\n M pnpm-lock.yaml\n' }, [CHECKOUT]: { code: 1 } });
    expect(await syncCheckout(a.exec, ROOT, OFF)).toEqual({ kind: 'skipped', reason: 'diverged' });
    expect(names(a)).not.toContain(FF);
    const b = fakeExec({ ...base, [CHANGED]: {}, [STATUS]: {}, [FF]: { code: 128 } });
    expect(await syncCheckout(b.exec, ROOT, OFF)).toEqual({ kind: 'skipped', reason: 'diverged' });
  });
  it('syncCheckout runs no checkout when the changed file list is empty', async () => {
    const f = fakeExec({ ...base, [CHANGED]: { stdout: '\n' }, [STATUS]: {}, [FF]: {}, [DIFF]: {} });
    await syncCheckout(f.exec, ROOT, OFF);
    expect(names(f).some((c) => c.startsWith('git checkout'))).toBe(false);
    expect(names(f)).toContain(FF);
  });
  it('syncCheckout reports current without any checkout when HEAD equals origin main', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'aaa\n' } });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toEqual({ kind: 'current', sha: 'aaa' });
    expect(names(f).some((c) => c.startsWith('git checkout'))).toBe(false);
  });
  it('syncCheckout skips when fetch fails', async () => {
    const f = fakeExec({ ...main, [FETCH]: { code: 1, stderr: 'offline' } });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toEqual({ kind: 'skipped', reason: 'fetch-failed' });
    expect(f.calls).toHaveLength(1);
  });
  it('syncCheckout skips when diverged', async () => {
    const a = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: { code: 1 }, [CHANGED]: {}, [STATUS]: {} });
    expect(await syncCheckout(a.exec, ROOT, OFF)).toEqual({ kind: 'skipped', reason: 'diverged' });
    expect(a.calls.some((c) => c.args[0] === 'merge' && c.args[1] === '--ff-only')).toBe(false);
    const b = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [CHANGED]: {}, [STATUS]: {}, [FF]: { code: 128 } });
    expect(await syncCheckout(b.exec, ROOT, OFF)).toEqual({ kind: 'skipped', reason: 'diverged' });
  });
  it('syncCheckout reports current when HEAD equals origin main', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'aaa\n' } });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toEqual({ kind: 'current', sha: 'aaa' });
    expect(f.calls.every((c) => c.cwd === ROOT && c.cmd === 'git')).toBe(true);
  });
  it('syncCheckout fast-forwards and reports lockfile change', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [CHANGED]: {}, [STATUS]: {}, [FF]: {}, [DIFF]: { stdout: 'web/a.ts\npnpm-lock.yaml\n' } });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: true });
  });
  it('syncCheckout fast-forwards and reports no lockfile change', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [CHANGED]: {}, [STATUS]: {}, [FF]: {}, [DIFF]: { stdout: 'web/a.ts\nweb/pnpm-lock.yaml.md\n' } });
    expect(await syncCheckout(f.exec, ROOT, OFF)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: false });
  });
});

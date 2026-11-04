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
const main = { [FETCH]: {} };

describe('syncCheckout', () => {
  const base = { ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {} };
  const names = (f: ReturnType<typeof fakeExec>) => f.calls.map((c) => `${c.cmd} ${c.args.join(' ')}`);
  it('syncCheckout checks out the files changed in HEAD..origin/main before fast-forwarding', async () => {
    const f = fakeExec({ ...base, [CHANGED]: { stdout: 'web/a.ts\npnpm-lock.yaml\n' }, [CHECKOUT]: {}, [FF]: {}, [DIFF]: { stdout: 'pnpm-lock.yaml\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: true });
    const n = names(f);
    expect(n.indexOf(CHANGED)).toBeGreaterThanOrEqual(0);
    expect(n.indexOf(CHECKOUT)).toBeGreaterThan(n.indexOf(CHANGED));
    expect(n.indexOf(FF)).toBeGreaterThan(n.indexOf(CHECKOUT));
  });
  it('syncCheckout fast-forwards a dirty tree and leaves untouched local edits', async () => {
    const f = fakeExec({ ...base, [CHANGED]: { stdout: 'web/a.ts\npnpm-lock.yaml\n' }, [CHECKOUT]: {}, [FF]: {}, [DIFF]: { stdout: 'web/a.ts\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toMatchObject({ kind: 'updated', from: 'aaa', to: 'bbb' });
    expect(names(f).some((c) => c.includes('status') || c.includes('stash') || c.includes('reset'))).toBe(false);
  });
  it('syncCheckout fast-forwards from any branch that is an ancestor of origin main', async () => {
    const f = fakeExec({ ...base, [CHANGED]: {}, [FF]: {}, [DIFF]: { stdout: 'web/a.ts\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toMatchObject({ kind: 'updated' });
    expect(names(f).some((c) => c.includes('--abbrev-ref'))).toBe(false);
  });
  it('syncCheckout skips as diverged when HEAD is not an ancestor', async () => {
    const f = fakeExec({ ...base, [ANC]: { code: 1 } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'diverged' });
    expect(names(f).some((c) => c.startsWith('git checkout') || c === FF)).toBe(false);
  });
  it('syncCheckout skips as diverged when the checkout or the merge fails', async () => {
    const a = fakeExec({ ...base, [CHANGED]: { stdout: 'web/a.ts\npnpm-lock.yaml\n' }, [CHECKOUT]: { code: 1 } });
    expect(await syncCheckout(a.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'diverged' });
    expect(names(a)).not.toContain(FF);
    const b = fakeExec({ ...base, [CHANGED]: {}, [FF]: { code: 128 } });
    expect(await syncCheckout(b.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'diverged' });
  });
  it('syncCheckout runs no checkout when the changed file list is empty', async () => {
    const f = fakeExec({ ...base, [CHANGED]: { stdout: '\n' }, [FF]: {}, [DIFF]: {} });
    await syncCheckout(f.exec, ROOT);
    expect(names(f).some((c) => c.startsWith('git checkout'))).toBe(false);
    expect(names(f)).toContain(FF);
  });
  it('syncCheckout reports current without any checkout when HEAD equals origin main', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'aaa\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'current', sha: 'aaa' });
    expect(names(f).some((c) => c.startsWith('git checkout'))).toBe(false);
  });
  it('syncCheckout skips when fetch fails', async () => {
    const f = fakeExec({ ...main, [FETCH]: { code: 1, stderr: 'offline' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'fetch-failed' });
    expect(f.calls).toHaveLength(1);
  });
  it('syncCheckout skips when diverged', async () => {
    const a = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: { code: 1 } });
    expect(await syncCheckout(a.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'diverged' });
    expect(a.calls.some((c) => c.args[0] === 'merge' && c.args[1] === '--ff-only')).toBe(false);
    const b = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [CHANGED]: {}, [FF]: { code: 128 } });
    expect(await syncCheckout(b.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'diverged' });
  });
  it('syncCheckout reports current when HEAD equals origin main', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'aaa\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'current', sha: 'aaa' });
    expect(f.calls.every((c) => c.cwd === ROOT && c.cmd === 'git')).toBe(true);
  });
  it('syncCheckout fast-forwards and reports lockfile change', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [CHANGED]: {}, [FF]: {}, [DIFF]: { stdout: 'web/a.ts\npnpm-lock.yaml\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: true });
  });
  it('syncCheckout fast-forwards and reports no lockfile change', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [CHANGED]: {}, [FF]: {}, [DIFF]: { stdout: 'web/a.ts\nweb/pnpm-lock.yaml.md\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: false });
  });
});

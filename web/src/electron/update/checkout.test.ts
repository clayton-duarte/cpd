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

const BRANCH = 'git rev-parse --abbrev-ref HEAD';
const STATUS = 'git status --porcelain';
const FETCH = 'git fetch origin';
const HEAD = 'git rev-parse HEAD';
const ORIGIN = 'git rev-parse origin/main';
const ANC = 'git merge-base --is-ancestor HEAD origin/main';
const FF = 'git merge --ff-only origin/main';
const DIFF = 'git diff --name-only aaa bbb';
const main = { [BRANCH]: { stdout: 'main\n' }, [STATUS]: { stdout: '' }, [FETCH]: {} };

describe('syncCheckout', () => {
  it('syncCheckout skips on another branch', async () => {
    const f = fakeExec({ [BRANCH]: { stdout: 'feature\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'branch' });
    expect(f.calls).toHaveLength(1);
  });
  it('syncCheckout skips on a dirty tree', async () => {
    const f = fakeExec({ [BRANCH]: { stdout: 'main\n' }, [STATUS]: { stdout: ' M a.ts\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'dirty' });
    expect(f.calls.map((c) => c.args[0])).toEqual(['rev-parse', 'status']);
  });
  it('syncCheckout skips when fetch fails', async () => {
    const f = fakeExec({ ...main, [FETCH]: { code: 1, stderr: 'offline' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'fetch-failed' });
    expect(f.calls).toHaveLength(3);
  });
  it('syncCheckout skips when diverged', async () => {
    const a = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: { code: 1 } });
    expect(await syncCheckout(a.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'diverged' });
    expect(a.calls.some((c) => c.args[0] === 'merge' && c.args[1] === '--ff-only')).toBe(false);
    const b = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [FF]: { code: 128 } });
    expect(await syncCheckout(b.exec, ROOT)).toEqual({ kind: 'skipped', reason: 'diverged' });
  });
  it('syncCheckout reports current when HEAD equals origin main', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'aaa\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'current', sha: 'aaa' });
    expect(f.calls.every((c) => c.cwd === ROOT && c.cmd === 'git')).toBe(true);
  });
  it('syncCheckout fast-forwards and reports lockfile change', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [FF]: {}, [DIFF]: { stdout: 'web/a.ts\npnpm-lock.yaml\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: true });
  });
  it('syncCheckout fast-forwards and reports no lockfile change', async () => {
    const f = fakeExec({ ...main, [HEAD]: { stdout: 'aaa\n' }, [ORIGIN]: { stdout: 'bbb\n' }, [ANC]: {}, [FF]: {}, [DIFF]: { stdout: 'web/a.ts\nweb/pnpm-lock.yaml.md\n' } });
    expect(await syncCheckout(f.exec, ROOT)).toEqual({ kind: 'updated', from: 'aaa', to: 'bbb', lockfileChanged: false });
  });
});

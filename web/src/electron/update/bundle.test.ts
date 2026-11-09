import { describe, expect, it } from 'vitest';
import { buildBundle, previousBuildDirs, readBuiltSha, swapBundle } from './bundle';
import { fakeExec, fakeFs } from './fakes';

const ROOT = '/repo';
const CUR = `${ROOT}/web/dist-electron`;
const NEXT = `${ROOT}/web/dist-electron.next`;
const INSTALL = 'pnpm install --frozen-lockfile';
const BUILD = 'pnpm --filter cpd-web-client run build:electron';

describe('buildBundle', () => {
  it('buildBundle removes a stale next dir before building', async () => {
    const fs = fakeFs({ [`${NEXT}/stale.js`]: 'old' });
    const f = fakeExec({ [BUILD]: {} });
    let sawStale: boolean | null = null;
    const exec: typeof f.exec = (c, a, o) => {
      sawStale = fs.exists(`${NEXT}/stale.js`);
      return f.exec(c, a, o);
    };
    expect(await buildBundle({ exec, fs }, ROOT, { lockfileChanged: false, sha: 's1' })).toEqual({ ok: true });
    expect(sawStale).toBe(false);
  });
  it('buildBundle skips pnpm install when the lockfile is unchanged', async () => {
    const f = fakeExec({ [BUILD]: {} });
    await buildBundle({ exec: f.exec, fs: fakeFs() }, ROOT, { lockfileChanged: false, sha: 's1' });
    expect(f.calls.map((c) => c.args)).toEqual([['--filter', 'cpd-web-client', 'run', 'build:electron']]);
  });
  it('buildBundle runs pnpm install first when the lockfile changed', async () => {
    const f = fakeExec({ [INSTALL]: {}, [BUILD]: {} });
    await buildBundle({ exec: f.exec, fs: fakeFs() }, ROOT, { lockfileChanged: true, sha: 's1' });
    expect(f.calls.map((c) => c.args[0])).toEqual(['install', '--filter']);
    expect(f.calls[0]).toMatchObject({ cmd: 'pnpm', cwd: ROOT });
  });
  it('buildBundle builds into dist-electron.next via CPD_ELECTRON_OUT', async () => {
    const f = fakeExec({ [BUILD]: {} });
    await buildBundle({ exec: f.exec, fs: fakeFs() }, ROOT, { lockfileChanged: false, sha: 's1' });
    expect(f.calls[0]).toMatchObject({ cmd: 'pnpm', cwd: ROOT, env: { CPD_ELECTRON_OUT: 'dist-electron.next' } });
  });
  it('buildBundle returns the log when the build fails', async () => {
    const fs = fakeFs();
    const f = fakeExec({ [BUILD]: { code: 1, stdout: 'out-text', stderr: 'err-text' } });
    const r = await buildBundle({ exec: f.exec, fs }, ROOT, { lockfileChanged: false, sha: 's1' });
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.log).toContain('err-text');
      expect(r.log).toContain('out-text');
    }
    expect(fs.exists(`${NEXT}/.built-sha`)).toBe(false);
  });
  it('buildBundle stamps the built sha', async () => {
    const fs = fakeFs();
    const f = fakeExec({ [BUILD]: {} });
    await buildBundle({ exec: f.exec, fs }, ROOT, { lockfileChanged: false, sha: 'abc123' });
    expect(fs.files.get(`${NEXT}/.built-sha`)).toBe('abc123');
  });
});

describe('readBuiltSha', () => {
  it('readBuiltSha returns null without a stamp', () => {
    expect(readBuiltSha(fakeFs(), ROOT)).toBeNull();
  });
  it('readBuiltSha returns the stamped sha', () => {
    expect(readBuiltSha(fakeFs({ [`${CUR}/.built-sha`]: 'abc123\n' }), ROOT)).toBe('abc123');
  });
});

const both = (curMain: string, nextMain: string, curPre: string, nextPre: string) => ({
  [`${CUR}/main.js`]: curMain,
  [`${CUR}/preload.cjs`]: curPre,
  [`${CUR}/marker`]: 'cur',
  [`${NEXT}/main.js`]: nextMain,
  [`${NEXT}/preload.cjs`]: nextPre,
  [`${NEXT}/marker`]: 'next',
});

describe('swapBundle', () => {
  it('swapBundle replaces dist-electron with the next build', () => {
    const fs = fakeFs(both('m', 'm', 'p', 'p'));
    swapBundle(fs, ROOT);
    expect(fs.files.get(`${CUR}/marker`)).toBe('next');
    expect(fs.exists(NEXT)).toBe(false);
  });
  it('swapBundle reports mainChanged when main.js differs', () => {
    expect(swapBundle(fakeFs(both('m1', 'm2', 'p', 'p')), ROOT)).toEqual({ mainChanged: true });
  });
  it('swapBundle reports mainChanged when preload.cjs differs', () => {
    expect(swapBundle(fakeFs(both('m', 'm', 'p1', 'p2')), ROOT)).toEqual({ mainChanged: true });
  });
  it('swapBundle reports unchanged when both match', () => {
    expect(swapBundle(fakeFs(both('m', 'm', 'p', 'p')), ROOT)).toEqual({ mainChanged: false });
  });
  it('swapBundle keeps the previous build on disk', () => {
    const fs = fakeFs(both('m', 'm', 'p', 'p'));
    const [head1] = previousBuildDirs(ROOT);
    swapBundle(fs, ROOT);
    // The displaced prod build (HEAD-1) is kept, not deleted, so a bad build can roll back.
    expect(fs.files.get(`${head1}/marker`)).toBe('cur');
  });
  it('keeps at most the last two previous builds', () => {
    const [head1, head2] = previousBuildDirs(ROOT);
    const fs = fakeFs({ [`${CUR}/marker`]: 'g0', [`${NEXT}/marker`]: 'g1' });
    swapBundle(fs, ROOT); // g1 current, g0 HEAD-1
    fs.writeText(`${NEXT}/marker`, 'g2');
    swapBundle(fs, ROOT); // g2 current, g1 HEAD-1, g0 HEAD-2
    expect(fs.files.get(`${head1}/marker`)).toBe('g1');
    expect(fs.files.get(`${head2}/marker`)).toBe('g0');
    fs.writeText(`${NEXT}/marker`, 'g3');
    swapBundle(fs, ROOT); // g3 current, g2 HEAD-1, g1 HEAD-2, g0 pruned
    expect(fs.files.get(`${CUR}/marker`)).toBe('g3');
    expect(fs.files.get(`${head1}/marker`)).toBe('g2');
    expect(fs.files.get(`${head2}/marker`)).toBe('g1');
    // Only two previous builds are kept: the one two generations back is gone.
    const g0Kept = [...fs.files.entries()].some(([p, v]) => p.endsWith('/marker') && v === 'g0');
    expect(g0Kept).toBe(false);
  });
  it('swapBundle rolls back when the second rename fails', () => {
    const fs = fakeFs(both('m', 'm', 'p', 'p'), (from) => from === NEXT);
    expect(() => swapBundle(fs, ROOT)).toThrow(/rename failed/);
    expect(fs.files.get(`${CUR}/marker`)).toBe('cur');
    expect(fs.files.get(`${NEXT}/marker`)).toBe('next');
  });
});

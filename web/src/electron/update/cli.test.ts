import { describe, expect, it } from 'vitest';
import { countRunningAttempts, installCli, readInstalledSha } from './cli';
import { fakeExec, fakeFs } from './fakes';
import type { UpdateTimers } from './types';

const ROOT = '/repo';
const STAMP = `${ROOT}/target/.cli-installed-sha`;
const PLANS = 'cpd plan list';
const LIST = (id: string) => `cpd attempt list --active --plan ${id}`;
const BUILD = 'scripts/cargo.sh build --release --locked -p cpd';
const INSTALL = 'scripts/install-cpd.sh';
const json = (v: unknown) => ({ stdout: JSON.stringify(v) });
const idle = { [PLANS]: json({ plans: [] }) };

function manualTimers() {
  const pending: { fn: () => void; ms: number }[] = [];
  const timers: UpdateTimers = {
    setTimeout: (fn, ms) => {
      pending.push({ fn, ms });
      return pending.length;
    },
    clearTimeout: () => {},
  };
  return { timers, pending };
}
const flush = async () => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};

describe('countRunningAttempts', () => {
  it('countRunningAttempts sums running attempts across plans', async () => {
    const f = fakeExec({
      [PLANS]: json({ plans: [{ id: 'a' }, { id: 'b' }] }),
      [LIST('a')]: json({ attempts: [{ status: 'running' }, { status: 'running' }] }),
      [LIST('b')]: json({ attempts: [{ status: 'running' }] }),
    });
    expect(await countRunningAttempts(f.exec, ROOT)).toBe(3);
    expect(f.calls.every((c) => c.cwd === ROOT)).toBe(true);
  });
  it('countRunningAttempts ignores attempts that are not running', async () => {
    const f = fakeExec({
      [PLANS]: json({ plans: [{ id: 'a' }] }),
      [LIST('a')]: json({ attempts: [{ status: 'running' }, { status: 'submitted' }, { status: 'failed' }] }),
    });
    expect(await countRunningAttempts(f.exec, ROOT)).toBe(1);
  });
  it('countRunningAttempts throws when cpd fails', async () => {
    const f = fakeExec({ [PLANS]: { code: 1, stderr: 'boom' } });
    await expect(countRunningAttempts(f.exec, ROOT)).rejects.toThrow(/plan list/);
  });
  it('countRunningAttempts throws on malformed JSON', async () => {
    const f = fakeExec({ [PLANS]: { stdout: 'not json' } });
    await expect(countRunningAttempts(f.exec, ROOT)).rejects.toThrow(/plan list/);
  });
});

describe('installCli', () => {
  it('installCli builds release before installing', async () => {
    const f = fakeExec({ [BUILD]: {}, ...idle, [INSTALL]: {} });
    const { timers } = manualTimers();
    expect(await installCli({ exec: f.exec, fs: fakeFs(), timers, log: () => {} }, ROOT, 's1')).toEqual({ ok: true });
    const keys = f.calls.map((c) => [c.cmd, ...c.args].join(' '));
    expect(keys[0]).toBe(BUILD);
    expect(keys[keys.length - 1]).toBe(INSTALL);
    expect(f.calls.every((c) => c.cwd === ROOT)).toBe(true);
  });
  it('installCli waits while attempts run and installs when idle', async () => {
    let n = 0;
    const f = fakeExec({
      [BUILD]: {},
      [PLANS]: () => json({ plans: [{ id: 'a' }] }),
      [LIST('a')]: () => json({ attempts: n++ === 0 ? [{ status: 'running' }] : [] }),
      [INSTALL]: {},
    });
    const { timers, pending } = manualTimers();
    const logs: string[] = [];
    const p = installCli({ exec: f.exec, fs: fakeFs(), timers, log: (l) => logs.push(l), retryMs: 1234 }, ROOT, 's1');
    await flush();
    expect(pending.map((t) => t.ms)).toEqual([1234]);
    expect(logs.length).toBeGreaterThan(0);
    expect(f.calls.some((c) => c.cmd === INSTALL)).toBe(false);
    pending[0]?.fn();
    expect(await p).toEqual({ ok: true });
    expect(f.calls.some((c) => c.cmd === INSTALL)).toBe(true);
  });
  it('installCli returns the log when the build fails', async () => {
    const f = fakeExec({ [BUILD]: { code: 101, stderr: 'compile error', stdout: 'out' } });
    const { timers } = manualTimers();
    const r = await installCli({ exec: f.exec, fs: fakeFs(), timers, log: () => {} }, ROOT, 's1');
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.log).toContain('compile error');
    expect(f.calls).toHaveLength(1);
  });
  it('installCli returns the log when install-cpd fails', async () => {
    const fs = fakeFs();
    const f = fakeExec({ [BUILD]: {}, ...idle, [INSTALL]: { code: 2, stderr: 'install broke' } });
    const { timers } = manualTimers();
    const r = await installCli({ exec: f.exec, fs, timers, log: () => {} }, ROOT, 's1');
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.log).toContain('install broke');
    expect(fs.files.has(STAMP)).toBe(false);
  });
  it('installCli stamps the installed sha', async () => {
    const fs = fakeFs();
    const f = fakeExec({ [BUILD]: {}, ...idle, [INSTALL]: {} });
    const { timers } = manualTimers();
    await installCli({ exec: f.exec, fs, timers, log: () => {} }, ROOT, 'abc123');
    expect(fs.files.get(STAMP)).toBe('abc123');
  });
  it('installCli fails loud when counting attempts fails', async () => {
    const f = fakeExec({ [BUILD]: {}, [PLANS]: { code: 1, stderr: 'down' } });
    const { timers } = manualTimers();
    const r = await installCli({ exec: f.exec, fs: fakeFs(), timers, log: () => {} }, ROOT, 's1');
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.log).toMatch(/plan list/);
    expect(f.calls.some((c) => c.cmd === INSTALL)).toBe(false);
  });
  it('installCli installs the binary before waiting for running attempts', async () => {
    const NO_FOLLOW_UP = `${INSTALL} --no-follow-up`;
    let n = 0;
    const f = fakeExec({
      [BUILD]: {},
      [PLANS]: () => json({ plans: [{ id: 'a' }] }),
      [LIST('a')]: () => json({ attempts: n++ === 0 ? [{ status: 'running' }] : [] }),
      [NO_FOLLOW_UP]: {},
      [INSTALL]: {},
    });
    const { timers, pending } = manualTimers();
    const invoked = () => f.calls.map((c) => [c.cmd, ...c.args].join(' '));
    const p = installCli({ exec: f.exec, fs: fakeFs(), timers, log: () => {}, retryMs: 1234 }, ROOT, 's1');
    await flush();
    // The binary is installed at once, before any wait: only the follow-ups wait for zero running attempts.
    expect(invoked()).toContain(NO_FOLLOW_UP);
    expect(invoked()).not.toContain(INSTALL);
    expect(pending.map((t) => t.ms)).toEqual([1234]);
    // The full install (the follow-ups) runs once, only when no attempt is running.
    pending[0]?.fn();
    expect(await p).toEqual({ ok: true });
    expect(invoked().filter((k) => k === INSTALL)).toHaveLength(1);
  });
  it("update error log includes the refused call's error code and message", async () => {
    const f = fakeExec({
      [BUILD]: {},
      [PLANS]: {
        code: 3,
        stdout: JSON.stringify({
          ok: false,
          error: { code: 'plan_has_lead', message: 'plan app-boot already has a lead' },
        }),
        stderr: '',
      },
    });
    const { timers } = manualTimers();
    const r = await installCli({ exec: f.exec, fs: fakeFs(), timers, log: () => {} }, ROOT, 's1');
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.log).toContain('plan_has_lead');
      expect(r.log).toContain('plan app-boot already has a lead');
    }
  });
});

describe('readInstalledSha', () => {
  it('readInstalledSha returns the stamp or null', () => {
    expect(readInstalledSha(fakeFs({ [STAMP]: 'abc\n' }), ROOT)).toBe('abc');
    expect(readInstalledSha(fakeFs(), ROOT)).toBeNull();
  });
});

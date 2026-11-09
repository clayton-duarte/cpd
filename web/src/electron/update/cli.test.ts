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
const NO_FOLLOW_UP = `${INSTALL} --no-follow-up`;
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
  it("countRunningAttempts names the refused call's error code and message", async () => {
    const f = fakeExec({
      [PLANS]: {
        code: 3,
        stdout: JSON.stringify({
          ok: false,
          error: { code: 'plan_has_lead', message: 'plan app-boot already has a lead' },
        }),
        stderr: '',
      },
    });
    await expect(countRunningAttempts(f.exec, ROOT)).rejects.toThrow(/plan_has_lead/);
    await expect(countRunningAttempts(f.exec, ROOT)).rejects.toThrow(/plan app-boot already has a lead/);
  });
});

describe('installCli', () => {
  it('installCli builds release before installing', async () => {
    const f = fakeExec({ [BUILD]: {}, [NO_FOLLOW_UP]: {}, ...idle, [INSTALL]: {} });
    const { timers } = manualTimers();
    expect(await installCli({ exec: f.exec, fs: fakeFs(), timers, log: () => {} }, ROOT, 's1')).toEqual({ ok: true });
    const keys = f.calls.map((c) => [c.cmd, ...c.args].join(' '));
    expect(keys[0]).toBe(BUILD);
    expect(keys[keys.length - 1]).toBe(INSTALL);
    expect(f.calls.every((c) => c.cwd === ROOT)).toBe(true);
  });
  it('the CLI installs while Attempts are running', async () => {
    const f = fakeExec({
      [BUILD]: {},
      [PLANS]: () => json({ plans: [{ id: 'a' }] }),
      [LIST('a')]: () => json({ attempts: [{ status: 'running' }] }),
      [NO_FOLLOW_UP]: {},
      [INSTALL]: {},
    });
    const { timers, pending } = manualTimers();
    const invoked = () => f.calls.map((c) => [c.cmd, ...c.args].join(' '));
    let result: { ok: boolean } | null = null;
    void installCli({ exec: f.exec, fs: fakeFs(), timers, log: () => {} }, ROOT, 's1').then((r) => {
      result = r;
    });
    await flush();
    // The install never waits for the running Attempts to reach zero: no timer is armed, and the full
    // install (scripts/install-cpd.sh, the follow-ups included) runs at once and resolves.
    expect(pending).toEqual([]);
    expect(invoked()).toContain(INSTALL);
    expect(result).toEqual({ ok: true });
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
    const f = fakeExec({ [BUILD]: {}, [NO_FOLLOW_UP]: {}, ...idle, [INSTALL]: { code: 2, stderr: 'install broke' } });
    const { timers } = manualTimers();
    const r = await installCli({ exec: f.exec, fs, timers, log: () => {} }, ROOT, 's1');
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.log).toContain('install broke');
    expect(fs.files.has(STAMP)).toBe(false);
  });
  it('installCli stamps the installed sha', async () => {
    const fs = fakeFs();
    const f = fakeExec({ [BUILD]: {}, [NO_FOLLOW_UP]: {}, ...idle, [INSTALL]: {} });
    const { timers } = manualTimers();
    await installCli({ exec: f.exec, fs, timers, log: () => {} }, ROOT, 'abc123');
    expect(fs.files.get(STAMP)).toBe('abc123');
  });
});

describe('readInstalledSha', () => {
  it('readInstalledSha returns the stamp or null', () => {
    expect(readInstalledSha(fakeFs({ [STAMP]: 'abc\n' }), ROOT)).toBe('abc');
    expect(readInstalledSha(fakeFs(), ROOT)).toBeNull();
  });
});

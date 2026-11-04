import { describe, expect, it } from 'vitest';
import { ensureCli } from './bootstrap';
import { fakeExec, fakeFs } from './fakes';
import type { BootstrapStatus } from './types';

const HOME = '/home/u';
const ROOT = '/repo';
const BIN = `${HOME}/.local/bin/cpd`;
const LOG = `${HOME}/.cpd/run/cli-bootstrap.log`;
const INSTALL = 'scripts/install-cpd.sh';

function run(o: { platform?: string; root?: string | null; files?: Record<string, string>; table?: Parameters<typeof fakeExec>[0] }) {
  const fs = fakeFs(o.files ?? {});
  const f = fakeExec(o.table ?? {});
  const statuses: BootstrapStatus[] = [];
  const result = ensureCli({ platform: o.platform ?? 'darwin', home: HOME, root: o.root === undefined ? ROOT : o.root, exec: f.exec, fs, onStatus: (s) => statuses.push(s) });
  return { result, statuses, fs, f };
}

describe('ensureCli', () => {
  it('ensureCli is ready at once off darwin', async () => {
    const r = run({ platform: 'linux' });
    expect(await r.result).toEqual({ state: 'ready' });
    expect(r.statuses).toEqual([{ state: 'ready' }]);
    expect(r.f.calls).toEqual([]);
  });
  it('ensureCli is ready at once when the binary exists', async () => {
    const r = run({ files: { [BIN]: 'x' } });
    expect(await r.result).toEqual({ state: 'ready' });
    expect(r.statuses).toEqual([{ state: 'ready' }]);
    expect(r.f.calls).toEqual([]);
  });
  it('ensureCli installs when the binary is missing', async () => {
    const fs = fakeFs();
    const statuses: BootstrapStatus[] = [];
    const f = fakeExec({ [INSTALL]: () => (fs.writeText(BIN, 'x'), {}) });
    const res = await ensureCli({ platform: 'darwin', home: HOME, root: ROOT, exec: f.exec, fs, onStatus: (s) => statuses.push(s) });
    expect(res).toEqual({ state: 'ready' });
    expect(statuses).toEqual([{ state: 'installing' }, { state: 'ready' }]);
    expect(f.calls).toEqual([{ cmd: INSTALL, args: [], cwd: ROOT, env: undefined }]);
  });
  it('ensureCli fails when no checkout is found', async () => {
    const r = run({ root: null });
    const expected = { state: 'failed', error: 'cpd checkout not found: set CPD_CHECKOUT', logPath: LOG };
    expect(await r.result).toEqual(expected);
    expect(r.statuses).toEqual([expected]);
    expect(r.f.calls).toEqual([]);
  });
  it('ensureCli fails with a log path when the install fails', async () => {
    const r = run({ table: { [INSTALL]: { code: 1, stdout: 'so', stderr: 'first\nreal error\n\n' } } });
    const res = await r.result;
    expect(res).toEqual({ state: 'failed', error: 'real error', logPath: LOG });
    expect(r.statuses).toEqual([{ state: 'installing' }, res]);
    const log = r.fs.files.get(LOG) ?? '';
    expect(log).toContain('so');
    expect(log).toContain('real error');
  });
  it('ensureCli fails when the binary is still missing after install', async () => {
    const r = run({ table: { [INSTALL]: {} } });
    const res = await r.result;
    expect(res.state).toBe('failed');
    expect(r.statuses).toEqual([{ state: 'installing' }, res]);
  });
});

import { describe, expect, it } from 'vitest';
import { DRIFT_STALE_MS, readDrift, rejectDrift } from './drift';
import { fakeExec } from './update/fakes';

const ROOT = '/repo';
const STATUS = 'git status --porcelain --untracked-files=all';
const LOG = 'git log --format=%H%x09%cI origin/main..HEAD';
const HEAD_DATE = 'git log -1 --format=%cI HEAD';
const AHEAD = 'git rev-list --count origin/main..HEAD';
const RESET = 'git reset --hard origin/main';
const NOW = Date.UTC(2026, 10, 6, 12, 0, 0);
const STAMP = '2026-11-06T12-00-00-000Z';
const REF = `refs/cpd/preview-rejected/${STAMP}`;
const UPDATE_REF = `git update-ref ${REF} HEAD`;
const STASH = `git stash push --include-untracked -m preview-rejected-${STAMP}`;
const iso = (ms: number) => new Date(ms).toISOString();
const names = (f: ReturnType<typeof fakeExec>) => f.calls.map((c) => `${c.cmd} ${c.args.join(' ')}`);

const read = (table: Record<string, Parameters<typeof fakeExec>[0][string]>, mtimes: Record<string, number | null> = {}, now = NOW) =>
  readDrift({ exec: fakeExec(table).exec, mtimeMs: (p) => mtimes[p] ?? null, now: () => now }, ROOT);

describe('readDrift', () => {
  it('readDrift reports no drift for a clean checkout at origin/main', async () => {
    expect(await read({ [STATUS]: {}, [LOG]: {} })).toEqual({ count: 0, oldestAt: null, stale: false, entries: [] });
  });

  it('readDrift counts dirty and untracked files with their mtimes', async () => {
    const a = NOW - 3_000;
    const b = NOW - 2_000;
    const c = NOW - 1_000;
    const r = await read(
      { [STATUS]: { stdout: ' M web/a.ts\n?? web/new.ts\nR  old.ts -> web/renamed.ts\n' }, [LOG]: {} },
      { '/repo/web/a.ts': a, '/repo/web/new.ts': b, '/repo/web/renamed.ts': c },
    );
    expect(r.entries).toEqual([
      { kind: 'dirty', ref: 'web/a.ts', at: iso(a) },
      { kind: 'untracked', ref: 'web/new.ts', at: iso(b) },
      { kind: 'dirty', ref: 'web/renamed.ts', at: iso(c) },
    ]);
    expect(r).toMatchObject({ count: 3, oldestAt: iso(a), stale: false });
  });

  it('readDrift counts commits ahead of origin/main with their commit dates', async () => {
    const r = await read({ [STATUS]: {}, [LOG]: { stdout: 'abc123\t2026-11-06T10:00:00-05:00\ndef456\t2026-11-06T09:00:00-05:00\n' } });
    expect(r.entries).toEqual([
      { kind: 'commit', ref: 'abc123', at: '2026-11-06T10:00:00-05:00' },
      { kind: 'commit', ref: 'def456', at: '2026-11-06T09:00:00-05:00' },
    ]);
    expect(r.count).toBe(2);
    expect(r.oldestAt).toBe('2026-11-06T09:00:00-05:00');
  });

  it('readDrift uses the HEAD commit date for a deleted file', async () => {
    const r = await read({ [STATUS]: { stdout: ' D gone.ts\n' }, [LOG]: {}, [HEAD_DATE]: { stdout: '2026-11-05T08:00:00Z\n' } });
    expect(r.entries).toEqual([{ kind: 'dirty', ref: 'gone.ts', at: '2026-11-05T08:00:00Z' }]);
  });

  it('readDrift marks drift older than 24 hours stale', async () => {
    const old = NOW - DRIFT_STALE_MS;
    const r = await read({ [STATUS]: { stdout: ' M a.ts\n' }, [LOG]: {} }, { '/repo/a.ts': old });
    expect(r).toMatchObject({ count: 1, oldestAt: iso(old), stale: true });
  });

  it('readDrift keeps drift under 24 hours fresh', async () => {
    const recent = NOW - DRIFT_STALE_MS + 1;
    const r = await read({ [STATUS]: { stdout: ' M a.ts\n' }, [LOG]: {} }, { '/repo/a.ts': recent });
    expect(r).toMatchObject({ count: 1, oldestAt: iso(recent), stale: false });
  });

  it('readDrift throws the stderr of a failing git', async () => {
    await expect(read({ [STATUS]: { code: 128, stderr: 'fatal: not a repo' }, [LOG]: {} })).rejects.toThrow('fatal: not a repo');
  });
});

describe('rejectDrift', () => {
  it('rejectDrift keeps the commits on a ref, stashes the dirty files and resets to origin/main', async () => {
    const f = fakeExec({ [STATUS]: { stdout: ' M a.ts\n?? b.ts\n' }, [AHEAD]: { stdout: '2\n' }, [UPDATE_REF]: {}, [STASH]: {}, [RESET]: {} });
    expect(await rejectDrift({ exec: f.exec, now: () => NOW }, ROOT)).toEqual({ ref: REF, stashed: true });
    const n = names(f);
    expect(n).toEqual([STATUS, AHEAD, UPDATE_REF, STASH, RESET]);
    expect(f.calls.every((c) => c.cwd === ROOT)).toBe(true);
    expect(n.some((c) => c.startsWith('git fetch'))).toBe(false);
  });

  it('rejectDrift does nothing to a clean checkout', async () => {
    const f = fakeExec({ [STATUS]: {}, [AHEAD]: { stdout: '0\n' } });
    expect(await rejectDrift({ exec: f.exec, now: () => NOW }, ROOT)).toEqual({ ref: null, stashed: false });
    expect(names(f)).toEqual([STATUS, AHEAD]);
  });

  it('rejectDrift stops without resetting when the stash fails', async () => {
    const f = fakeExec({ [STATUS]: { stdout: ' M a.ts\n' }, [AHEAD]: { stdout: '0\n' }, [STASH]: { code: 1, stderr: 'stash boom' } });
    await expect(rejectDrift({ exec: f.exec, now: () => NOW }, ROOT)).rejects.toThrow('stash boom');
    expect(names(f)).not.toContain(RESET);
  });
});

import type { UpdateExec, UpdateExecResult, UpdateFs, UpdateTimers } from './types';

export interface FakeExec {
  exec: UpdateExec;
  calls: { cmd: string; args: string[]; cwd: string; env?: Record<string, string> }[];
}

type Answer = Partial<UpdateExecResult> | (() => Partial<UpdateExecResult>);

/** Answers from a table keyed 'cmd args'; an unlisted command fails the test. */
export function fakeExec(table: Record<string, Answer>): FakeExec {
  const calls: FakeExec['calls'] = [];
  const exec: UpdateExec = async (cmd, args, opts) => {
    calls.push({ cmd, args, cwd: opts.cwd, env: opts.env });
    const key = [cmd, ...args].join(' ');
    const answer = table[key];
    if (answer === undefined) throw new Error(`unexpected exec: ${key}`);
    const r = typeof answer === 'function' ? answer() : answer;
    return { code: 0, stdout: '', stderr: '', ...r };
  };
  return { exec, calls };
}

export interface FakeFs extends UpdateFs {
  files: Map<string, string>;
  renames: [string, string][];
}

/** In-memory fs: a path exists when it is a file or a directory prefix of one. */
export function fakeFs(initial: Record<string, string> = {}, failRename?: (from: string, to: string) => boolean): FakeFs {
  const files = new Map<string, string>(Object.entries(initial));
  const renames: [string, string][] = [];
  const under = (p: string) => [...files.keys()].filter((k) => k === p || k.startsWith(`${p}/`));
  return {
    files,
    renames,
    exists: (p) => under(p).length > 0,
    readText: (p) => files.get(p) ?? null,
    writeText: (p, t) => void files.set(p, t),
    rename(from, to) {
      if (failRename?.(from, to)) throw new Error(`rename failed: ${from} -> ${to}`);
      renames.push([from, to]);
      for (const k of under(from)) {
        const text = files.get(k) as string;
        files.delete(k);
        files.set(to + k.slice(from.length), text);
      }
    },
    remove(p) {
      for (const k of under(p)) files.delete(k);
    },
  };
}

export interface FakeTimers extends UpdateTimers {
  pending: { fn: () => void; ms: number; handle: number }[];
  /** Fires the oldest pending timer by hand. */
  fire(): void;
}

/** Manual timers: nothing runs until the test calls fire(). */
export function fakeTimers(): FakeTimers {
  const pending: FakeTimers['pending'] = [];
  let next = 1;
  return {
    pending,
    setTimeout(fn, ms) {
      const handle = next++;
      pending.push({ fn, ms, handle });
      return handle;
    },
    clearTimeout(handle) {
      const i = pending.findIndex((t) => t.handle === handle);
      if (i >= 0) pending.splice(i, 1);
    },
    fire() {
      const t = pending.shift();
      if (!t) throw new Error('no pending timer');
      t.fn();
    },
  };
}

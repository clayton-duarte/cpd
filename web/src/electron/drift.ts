import type { UpdateExec } from './update/types';

export const DRIFT_STALE_MS = 86_400_000;

export type DriftEntry = { readonly kind: 'dirty' | 'untracked' | 'commit'; readonly ref: string; readonly at: string };

export interface DriftSummary {
  readonly count: number;
  readonly oldestAt: string | null;
  readonly stale: boolean;
}

export interface DriftReport extends DriftSummary {
  readonly entries: readonly DriftEntry[];
}

async function git(exec: UpdateExec, root: string, args: string[]): Promise<string> {
  const r = await exec('git', args, { cwd: root });
  if (r.code !== 0) throw new Error(r.stderr.trim() || `git ${args.join(' ')} failed with ${r.code}`);
  return r.stdout;
}

const STATUS = ['status', '--porcelain', '--untracked-files=all'];

function statusFiles(out: string): { kind: 'dirty' | 'untracked'; path: string }[] {
  return out
    .split('\n')
    .filter((l) => l.length > 3)
    .map((l) => {
      const rest = l.slice(3);
      const arrow = rest.indexOf(' -> ');
      return { kind: l.startsWith('??') ? ('untracked' as const) : ('dirty' as const), path: arrow >= 0 ? rest.slice(arrow + 4) : rest };
    });
}

export async function readDrift(
  deps: { exec: UpdateExec; mtimeMs(path: string): number | null; now(): number },
  root: string,
): Promise<DriftReport> {
  const files = statusFiles(await git(deps.exec, root, STATUS));
  const commits = (await git(deps.exec, root, ['log', '--format=%H%x09%cI', 'origin/main..HEAD']))
    .split('\n')
    .filter((l) => l.trim() !== '')
    .map((l) => {
      const [ref, at] = l.split('\t');
      return { kind: 'commit' as const, ref, at };
    });
  let headDate: string | null = null;
  const entries: DriftEntry[] = [];
  for (const f of files) {
    const ms = deps.mtimeMs(`${root}/${f.path}`);
    if (ms === null) headDate ??= (await git(deps.exec, root, ['log', '-1', '--format=%cI', 'HEAD'])).trim();
    entries.push({ kind: f.kind, ref: f.path, at: ms === null ? (headDate as string) : new Date(ms).toISOString() });
  }
  entries.push(...commits);
  let oldestAt: string | null = null;
  for (const e of entries) if (oldestAt === null || Date.parse(e.at) < Date.parse(oldestAt)) oldestAt = e.at;
  const stale = oldestAt !== null && deps.now() - Date.parse(oldestAt) >= DRIFT_STALE_MS;
  return { count: entries.length, oldestAt, stale, entries };
}

export async function rejectDrift(
  deps: { exec: UpdateExec; now(): number },
  root: string,
): Promise<{ ref: string | null; stashed: boolean }> {
  const dirty = statusFiles(await git(deps.exec, root, STATUS)).length > 0;
  const ahead = Number((await git(deps.exec, root, ['rev-list', '--count', 'origin/main..HEAD'])).trim()) > 0;
  if (!dirty && !ahead) return { ref: null, stashed: false };
  const stamp = new Date(deps.now()).toISOString().replace(/[:.]/g, '-');
  let ref: string | null = null;
  if (ahead) {
    ref = `refs/cpd/preview-rejected/${stamp}`;
    await git(deps.exec, root, ['update-ref', ref, 'HEAD']);
  }
  if (dirty) await git(deps.exec, root, ['stash', 'push', '--include-untracked', '-m', `preview-rejected-${stamp}`]);
  await git(deps.exec, root, ['reset', '--hard', 'origin/main']);
  return { ref, stashed: dirty };
}

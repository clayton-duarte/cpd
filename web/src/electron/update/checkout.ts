import path from 'node:path';
import type { UpdateExec, UpdateFs } from './types';

export function resolveCheckout(deps: { env: Record<string, string | undefined>; bundleDir: string; fs: Pick<UpdateFs, 'exists'> }): string | null {
  const root = deps.env['CPD_CHECKOUT'] ?? path.resolve(deps.bundleDir, '..', '..');
  return deps.fs.exists(`${root}/scripts/install-cpd.sh`) && deps.fs.exists(`${root}/.git`) ? root : null;
}
export type SyncResult =
  | { kind: 'current'; sha: string }
  | { kind: 'updated'; from: string; to: string; lockfileChanged: boolean }
  | { kind: 'skipped'; reason: 'branch' | 'dirty' | 'fetch-failed' | 'diverged' | 'rebase-conflict' };
const lines = (out: string): string[] => out.split('\n').map((l) => l.trim()).filter(Boolean);
export async function syncCheckout(exec: UpdateExec, root: string, opts: { readonly rebase: boolean }): Promise<SyncResult> {
  const git = (...args: string[]) => exec('git', args, { cwd: root });
  if ((await git('fetch', 'origin')).code !== 0) return { kind: 'skipped', reason: 'fetch-failed' };
  const from = (await git('rev-parse', 'HEAD')).stdout.trim();
  const to = (await git('rev-parse', 'origin/main')).stdout.trim();
  if (from === to) return { kind: 'current', sha: from };
  const changed = lines((await git('diff', '--name-only', 'HEAD', 'origin/main')).stdout);
  const dirty = (await git('status', '--porcelain', '--untracked-files=all')).stdout.split('\n').filter((l) => l.length > 3);
  const entry = (l: string) => ({ untracked: l.startsWith('??'), file: l.slice(3).split(' -> ').pop() ?? '' });
  const entries = dirty.map(entry);
  const tracked = changed.filter((f) => entries.some((e) => !e.untracked && e.file === f));
  const untracked = changed.filter((f) => entries.some((e) => e.untracked && e.file === f));
  if (tracked.length > 0 && (await git('checkout', 'HEAD', '--', ...tracked)).code !== 0) return { kind: 'skipped', reason: 'diverged' };
  if (untracked.length > 0 && (await git('clean', '-f', '--', ...untracked)).code !== 0) return { kind: 'skipped', reason: 'diverged' };
  if ((await git('merge-base', '--is-ancestor', 'HEAD', 'origin/main')).code !== 0) {
    if (!opts.rebase) return { kind: 'skipped', reason: 'diverged' };
    if ((await git('rebase', '--autostash', 'origin/main')).code !== 0) {
      await git('rebase', '--abort');
      return { kind: 'skipped', reason: 'rebase-conflict' };
    }
  } else if ((await git('merge', '--ff-only', 'origin/main')).code !== 0) return { kind: 'skipped', reason: 'diverged' };
  const files = lines((await git('diff', '--name-only', from, to)).stdout);
  return { kind: 'updated', from, to, lockfileChanged: files.includes('pnpm-lock.yaml') };
}

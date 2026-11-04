import path from 'node:path';
import type { UpdateExec, UpdateFs } from './types';

export function resolveCheckout(deps: { env: Record<string, string | undefined>; bundleDir: string; fs: Pick<UpdateFs, 'exists'> }): string | null {
  const root = deps.env['CPD_CHECKOUT'] ?? path.resolve(deps.bundleDir, '..', '..');
  return deps.fs.exists(`${root}/scripts/install-cpd.sh`) && deps.fs.exists(`${root}/.git`) ? root : null;
}
export type SyncResult =
  | { kind: 'current'; sha: string }
  | { kind: 'updated'; from: string; to: string; lockfileChanged: boolean }
  | { kind: 'skipped'; reason: 'branch' | 'dirty' | 'fetch-failed' | 'diverged' };
export async function syncCheckout(exec: UpdateExec, root: string): Promise<SyncResult> {
  const git = (...args: string[]) => exec('git', args, { cwd: root });
  if ((await git('rev-parse', '--abbrev-ref', 'HEAD')).stdout.trim() !== 'main') return { kind: 'skipped', reason: 'branch' };
  if ((await git('status', '--porcelain')).stdout.trim() !== '') return { kind: 'skipped', reason: 'dirty' };
  if ((await git('fetch', 'origin')).code !== 0) return { kind: 'skipped', reason: 'fetch-failed' };
  const from = (await git('rev-parse', 'HEAD')).stdout.trim();
  const to = (await git('rev-parse', 'origin/main')).stdout.trim();
  if (from === to) return { kind: 'current', sha: from };
  if ((await git('merge-base', '--is-ancestor', 'HEAD', 'origin/main')).code !== 0) return { kind: 'skipped', reason: 'diverged' };
  if ((await git('merge', '--ff-only', 'origin/main')).code !== 0) return { kind: 'skipped', reason: 'diverged' };
  const names = (await git('diff', '--name-only', from, to)).stdout.split('\n').map((l) => l.trim());
  return { kind: 'updated', from, to, lockfileChanged: names.includes('pnpm-lock.yaml') };
}

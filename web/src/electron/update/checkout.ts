import path from 'node:path';
import type { UpdateExec, UpdateFs } from './types';

export function resolveCheckout(deps: { env: Record<string, string | undefined>; bundleDir: string; fs: Pick<UpdateFs, 'exists'> }): string | null {
  const root = deps.env['CPD_CHECKOUT'] ?? path.resolve(deps.bundleDir, '..', '..');
  return deps.fs.exists(`${root}/scripts/install-cpd.sh`) && deps.fs.exists(`${root}/.git`) ? root : null;
}
export type SyncResult =
  | { kind: 'current'; sha: string }
  | { kind: 'updated'; from: string; to: string; lockfileChanged: boolean }
  | { kind: 'skipped'; reason: 'fetch-failed' | 'diverged' };
export async function syncCheckout(exec: UpdateExec, root: string): Promise<SyncResult> {
  const git = (...args: string[]) => exec('git', args, { cwd: root });
  if ((await git('fetch', 'origin')).code !== 0) return { kind: 'skipped', reason: 'fetch-failed' };
  throw new Error('not implemented');
}

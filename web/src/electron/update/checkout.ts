import type { UpdateExec, UpdateFs } from './types';

export function resolveCheckout(_deps: { env: Record<string, string | undefined>; bundleDir: string; fs: Pick<UpdateFs, 'exists'> }): string | null {
  throw new Error('not implemented');
}
export type SyncResult =
  | { kind: 'current'; sha: string }
  | { kind: 'updated'; from: string; to: string; lockfileChanged: boolean }
  | { kind: 'skipped'; reason: 'branch' | 'dirty' | 'fetch-failed' | 'diverged' };
export function syncCheckout(_exec: UpdateExec, _root: string): Promise<SyncResult> {
  throw new Error('not implemented');
}

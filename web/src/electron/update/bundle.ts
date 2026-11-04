import type { UpdateExec, UpdateFs } from './types';

export const BUILT_SHA_FILE = '.built-sha';
export function buildBundle(_deps: { exec: UpdateExec; fs: UpdateFs }, _root: string, _opts: { lockfileChanged: boolean; sha: string }): Promise<{ ok: true } | { ok: false; log: string }> {
  throw new Error('not implemented');
}
export function readBuiltSha(_fs: UpdateFs, _root: string): string | null {
  throw new Error('not implemented');
}
export function swapBundle(_fs: UpdateFs, _root: string): { mainChanged: boolean } {
  throw new Error('not implemented');
}

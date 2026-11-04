import type { UpdateExec, UpdateFs, UpdateTimers } from './types';

export function countRunningAttempts(_exec: UpdateExec, _root: string): Promise<number> {
  throw new Error('not implemented');
}
export function installCli(_deps: { exec: UpdateExec; fs: UpdateFs; timers: UpdateTimers; log: (line: string) => void; retryMs?: number }, _root: string, _sha: string): Promise<{ ok: true } | { ok: false; log: string }> {
  throw new Error('not implemented');
}
export function readInstalledSha(_fs: UpdateFs, _root: string): string | null {
  throw new Error('not implemented');
}

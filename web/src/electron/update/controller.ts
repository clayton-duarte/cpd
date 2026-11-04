import type { SyncResult } from './checkout';
import type { UpdateState } from './types';

export interface UpdaterDeps { sync(): Promise<SyncResult>; headSha(): Promise<string>; builtSha(): string | null; cliSha(): string | null; build(opts: { lockfileChanged: boolean; sha: string }): Promise<{ ok: true } | { ok: false; log: string }>; swap(): { mainChanged: boolean }; whenApplicable(): Promise<void>; reloadRenderer(): void; installCli(sha: string): Promise<{ ok: true } | { ok: false; log: string }>; onState(s: UpdateState): void; log(line: string): void }

export function createUpdater(_deps: UpdaterDeps): { check(): Promise<void> } {
  throw new Error('not implemented');
}

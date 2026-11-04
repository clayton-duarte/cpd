export function parseEventId(_line: string): number {
  throw new Error('not implemented');
}
export interface FollowProcess { onLine(cb: (line: string) => void): void; onExit(cb: (code: number | null) => void): void; kill(): void }
export type SpawnFollow = (args: string[]) => FollowProcess;
export function followMerges(_deps: { spawn: SpawnFollow; timers: import('./types').UpdateTimers; onMerged: () => void; onError: (e: Error) => void; restartMs?: number }): { stop(): void } {
  throw new Error('not implemented');
}

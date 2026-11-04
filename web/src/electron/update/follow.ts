import type { UpdateTimers } from './types';

export function parseEventId(line: string): number {
  const v: unknown = JSON.parse(line);
  if (typeof v !== 'object' || v === null) throw new Error('event line is not an object');
  const e = v as { id?: unknown; kind?: unknown };
  if (e.kind !== 'pr-merged') throw new Error(`unexpected event kind: ${String(e.kind)}`);
  if (typeof e.id !== 'number') throw new Error('event id is missing');
  return e.id;
}

export interface FollowProcess { onLine(cb: (line: string) => void): void; onExit(cb: (code: number | null) => void): void; kill(): void }
export type SpawnFollow = (args: string[]) => FollowProcess;

const BASE = ['events', 'follow', '--kind', 'pr-merged'];

export function followMerges(deps: { spawn: SpawnFollow; timers: UpdateTimers; onMerged: () => void; onError: (e: Error) => void; restartMs?: number }): { stop(): void } {
  const restartMs = deps.restartMs ?? 5000;
  let lastId: number | null = null;
  let stopped = false;
  let proc: FollowProcess | null = null;
  let timer: unknown = null;

  const start = (): void => {
    timer = null;
    proc = deps.spawn(lastId === null ? BASE : [...BASE, '--since', String(lastId)]);
    proc.onLine((line) => {
      if (stopped) return;
      try {
        lastId = parseEventId(line);
      } catch (e) {
        deps.onError(e instanceof Error ? e : new Error(String(e)));
        return;
      }
      deps.onMerged();
    });
    proc.onExit(() => {
      if (stopped) return;
      timer = deps.timers.setTimeout(start, restartMs);
    });
    deps.onMerged();
  };

  start();
  return {
    stop() {
      stopped = true;
      proc?.kill();
      if (timer !== null) deps.timers.clearTimeout(timer);
      timer = null;
    },
  };
}

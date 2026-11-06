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

export async function readDrift(
  _deps: { exec: UpdateExec; mtimeMs(path: string): number | null; now(): number },
  _root: string,
): Promise<DriftReport> {
  throw new Error('not implemented');
}

export async function rejectDrift(
  _deps: { exec: UpdateExec; now(): number },
  _root: string,
): Promise<{ ref: string | null; stashed: boolean }> {
  throw new Error('not implemented');
}

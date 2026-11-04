import type { BootstrapStatus, UpdateExec, UpdateFs } from './types';

export function ensureCli(_deps: { platform: string; home: string; root: string | null; exec: UpdateExec; fs: UpdateFs; onStatus: (s: BootstrapStatus) => void }): Promise<BootstrapStatus> {
  throw new Error('not implemented');
}

import type { SpawnFollow } from './follow';
import type { BootstrapStatus, UpdateExec, UpdateFs, UpdateState, UpdateTimers } from './types';

export interface AutoUpdateDeps { env: Record<string, string | undefined>; bundleDir: string; home: string; platform: string; exec: UpdateExec; fs: UpdateFs; timers: UpdateTimers; spawnFollow: SpawnFollow; isFocused: () => boolean; systemIdleSeconds: () => number; reloadRenderer: () => void; onState: (s: UpdateState) => void; onBootstrap: (s: BootstrapStatus) => void; log: (line: string) => void }

export function startAutoUpdate(_deps: AutoUpdateDeps): Promise<{ bootstrap: BootstrapStatus; stop(): void }> {
  throw new Error('not implemented');
}

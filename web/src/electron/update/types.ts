export interface UpdateExecResult { code: number; stdout: string; stderr: string }
export type UpdateExec = (cmd: string, args: string[], opts: { cwd: string; env?: Record<string, string> }) => Promise<UpdateExecResult>;
export interface UpdateFs { exists(p: string): boolean; readText(p: string): string | null; writeText(p: string, text: string): void; rename(from: string, to: string): void; remove(p: string): void }
export interface UpdateTimers { setTimeout(fn: () => void, ms: number): unknown; clearTimeout(handle: unknown): void }
export type BootstrapStatus = { state: 'ready' } | { state: 'installing' } | { state: 'failed'; error: string; logPath: string };
export type UpdateState = { phase: 'idle' } | { phase: 'syncing' } | { phase: 'building' } | { phase: 'waiting' } | { phase: 'restart-needed' } | { phase: 'error'; message: string };

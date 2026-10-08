import { closeSync, openSync, readFileSync, rmSync, writeSync } from "node:fs";

export type LockInfo = {
  pid: number;
  startedAt: string;
};

export type AcquireLockOptions = {
  /** Injectable for tests so we don't depend on real OS pids. */
  isPidAlive?: (pid: number) => boolean;
};

function lockPathFor(dbPath: string): string {
  return `${dbPath}.lock`;
}

/**
 * Check whether a process is alive via `process.kill(pid, 0)`.
 * - ESRCH -> dead, returns false.
 * - no throw, or EPERM -> alive (process exists, even if not ours), returns true.
 */
export function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === "ESRCH") return false;
    if (code === "EPERM") return true;
    return true;
  }
}

function readLock(path: string): LockInfo | undefined {
  try {
    return JSON.parse(readFileSync(path, "utf8")) as LockInfo;
  } catch {
    return undefined;
  }
}

function writeLock(path: string): void {
  const info: LockInfo = { pid: process.pid, startedAt: new Date().toISOString() };
  const fd = openSync(path, "wx");
  try {
    writeSync(fd, JSON.stringify(info));
  } finally {
    closeSync(fd);
  }
}

/**
 * Acquire a single-process ownership lock next to `dbPath`. Atomic creation via `wx` avoids a
 * TOCTOU race between checking existence and writing. If the lock already exists and its owner
 * is dead, it is reclaimed; if alive, throws an actionable error.
 */
export function acquireLock(dbPath: string, options: AcquireLockOptions = {}): void {
  const checkAlive = options.isPidAlive ?? isPidAlive;
  const path = lockPathFor(dbPath);

  try {
    writeLock(path);
    return;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
  }

  const existing = readLock(path);
  if (existing && checkAlive(existing.pid)) {
    throw new Error(
      `cpd-daemon: another daemon (pid ${existing.pid}) already owns ${dbPath}. ` +
        `This process (pid ${process.pid}) is exiting without starting -- the port may still be ` +
        `served by pid ${existing.pid}. Stop it first.`,
    );
  }

  // Stale lock (dead owner, or unreadable/corrupt file): remove and take ownership.
  rmSync(path, { force: true });
  writeLock(path);
}

/** Release the lock, but only if we own it (pid matches), so we never delete another owner's lock. */
export function releaseLock(dbPath: string): void {
  const path = lockPathFor(dbPath);
  const existing = readLock(path);
  if (!existing || existing.pid !== process.pid) return;
  rmSync(path, { force: true });
}

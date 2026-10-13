import { closeSync, mkdirSync, openSync, readFileSync, statSync, unlinkSync, writeSync } from "node:fs";
import { dirname } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { z } from "zod";
import { failure, timeout } from "./errors.ts";

const LockFile = z.object({ pid: z.number().int().positive(), at: z.string(), purpose: z.string() });
export type LockInfo = z.infer<typeof LockFile>;

export type LockOptions = { timeoutMs?: number; pollMs?: number };

/** A lock file that is still empty or unparseable after this long was abandoned mid-creation. */
const TORN_GRACE_MS = 2000;

export function pidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === "EPERM";
  }
}

type Holder = { kind: "live"; info: LockInfo } | { kind: "stale"; info: LockInfo | null } | { kind: "gone" } | { kind: "torn" };

/** Classify the current holder of `path`. */
function inspect(path: string): Holder {
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return { kind: "gone" };
    throw failure("lock_unreadable", `cannot read lock ${path}: ${(e as Error).message}`, { path });
  }
  let parsed: LockInfo | undefined;
  try {
    const r = LockFile.safeParse(JSON.parse(text));
    if (r.success) parsed = r.data;
  } catch {
    // unparseable: handled below
  }
  if (parsed) return pidAlive(parsed.pid) ? { kind: "live", info: parsed } : { kind: "stale", info: parsed };
  // Empty or garbage: the creator may be between open() and write().
  try {
    if (Date.now() - statSync(path).mtimeMs < TORN_GRACE_MS) return { kind: "torn" };
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return { kind: "gone" };
    throw e;
  }
  return { kind: "stale", info: null };
}

function tryCreate(path: string, content: string): boolean {
  let fd: number;
  try {
    fd = openSync(path, "wx");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "EEXIST") return false;
    throw e;
  }
  try {
    writeSync(fd, content);
  } finally {
    closeSync(fd);
  }
  return true;
}

/**
 * Take over a stale lock. Two waiters may find the same stale lock, so the removal itself
 * is serialised by a guard file; inside the guard the lock is re-inspected before unlinking.
 */
function takeOver(path: string, seen: Holder & { kind: "stale" }): boolean {
  const guard = `${path}.takeover`;
  const mine = JSON.stringify({ pid: process.pid, at: new Date().toISOString(), purpose: "takeover" });
  if (!tryCreate(guard, mine)) {
    const g = inspect(guard);
    if (g.kind === "stale") {
      try {
        unlinkSync(guard);
      } catch {
        // another waiter removed it first
      }
    }
    return false;
  }
  try {
    const again = inspect(path);
    if (again.kind === "stale") {
      process.stderr.write(
        `cpd: taking over stale lock ${path} (held by pid ${seen.info?.pid ?? "unknown"}, purpose ${seen.info?.purpose ?? "unknown"})\n`,
      );
      unlinkSync(path);
      return true;
    }
    return false;
  } finally {
    try {
      unlinkSync(guard);
    } catch {
      // already gone
    }
  }
}

/** Contract §4: exclusive-create lock, stale takeover, bounded wait, always released. */
export async function withLock<T>(
  path: string,
  purpose: string,
  fn: () => Promise<T> | T,
  { timeoutMs = 60000, pollMs = 25 }: LockOptions = {},
): Promise<T> {
  mkdirSync(dirname(path), { recursive: true });
  const deadline = Date.now() + timeoutMs;
  const content = JSON.stringify({ pid: process.pid, at: new Date().toISOString(), purpose });
  let last: Holder = { kind: "gone" };
  for (;;) {
    if (tryCreate(path, content)) break;
    last = inspect(path);
    if (last.kind === "stale") {
      if (takeOver(path, last)) continue;
    }
    if (Date.now() >= deadline) {
      throw timeout("lock_timeout", `timed out after ${timeoutMs} ms waiting for lock ${path}`, {
        path,
        holder: last.kind === "live" ? last.info : null,
      });
    }
    await sleep(pollMs);
  }
  try {
    return await fn();
  } finally {
    // Release only our own lock: never unlink one a taker created after we were deemed stale.
    try {
      if (readFileSync(path, "utf8") === content) unlinkSync(path);
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code !== "ENOENT") throw e;
    }
  }
}

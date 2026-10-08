import { describe, expect, it, afterEach } from "vitest";
import { mkdtempSync, rmSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { acquireLock, releaseLock, isPidAlive } from "./lock.ts";

function tempDbPath(): string {
  const dir = mkdtempSync(join(tmpdir(), "cpd-lock-test-"));
  return join(dir, "test.sqlite");
}

function deadPid(): number {
  // Spawn a trivial child and let it exit; its pid is then guaranteed not reused
  // within this fast test run (immediate wait() reaps it).
  const result = spawnSync(process.execPath, ["-e", "process.exit(0)"]);
  return result.pid ?? 999999;
}

describe("lock", () => {
  const dirs: string[] = [];

  afterEach(() => {
    for (const d of dirs.splice(0)) {
      rmSync(d, { recursive: true, force: true });
    }
  });

  it("creates the lockfile with our pid", () => {
    const dbPath = tempDbPath();
    dirs.push(dbPath.replace(/\/[^/]+$/, ""));

    acquireLock(dbPath);

    const lockPath = `${dbPath}.lock`;
    expect(existsSync(lockPath)).toBe(true);
    const contents = JSON.parse(readFileSync(lockPath, "utf8")) as { pid: number; startedAt: string };
    expect(contents.pid).toBe(process.pid);
    expect(typeof contents.startedAt).toBe("string");

    releaseLock(dbPath);
  });

  it("throws an actionable error when a live pid already owns the lock", () => {
    const dbPath = tempDbPath();
    dirs.push(dbPath.replace(/\/[^/]+$/, ""));

    acquireLock(dbPath);
    expect(() => acquireLock(dbPath)).toThrowError(
      new RegExp(`another daemon \\(pid ${process.pid}\\) already owns ${dbPath}`),
    );

    releaseLock(dbPath);
  });

  it("treats a lock with a dead pid as stale and takes it over", () => {
    const dbPath = tempDbPath();
    dirs.push(dbPath.replace(/\/[^/]+$/, ""));
    const pid = deadPid();

    acquireLock(dbPath, { isPidAlive: (p) => p !== pid });

    const lockPath = `${dbPath}.lock`;
    const contents = JSON.parse(readFileSync(lockPath, "utf8")) as { pid: number };
    expect(contents.pid).toBe(process.pid);

    releaseLock(dbPath);
  });

  it("release removes the file, and does not remove a lock owned by another pid", () => {
    const dbPath = tempDbPath();
    dirs.push(dbPath.replace(/\/[^/]+$/, ""));

    acquireLock(dbPath);
    releaseLock(dbPath);
    expect(existsSync(`${dbPath}.lock`)).toBe(false);
  });

  it("does not remove a lock owned by a different pid", () => {
    const dbPath = tempDbPath();
    dirs.push(dbPath.replace(/\/[^/]+$/, ""));
    const pid = deadPid();

    acquireLock(dbPath, { isPidAlive: (p) => p !== pid });
    // simulate a different owner having since taken it over
    const lockPath = `${dbPath}.lock`;
    const otherOwner = JSON.stringify({ pid: pid + 1, startedAt: new Date().toISOString() });
    writeFileSync(lockPath, otherOwner);

    releaseLock(dbPath);
    expect(existsSync(lockPath)).toBe(true);

    rmSync(lockPath, { force: true });
  });

  it("exports an injectable liveness check", () => {
    expect(typeof isPidAlive).toBe("function");
    expect(isPidAlive(process.pid)).toBe(true);
  });
});

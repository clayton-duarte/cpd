import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { withLock } from "../../src/lib/lock.ts";
import { tmpDir } from "../helpers/index.ts";

const LOCK_TS = fileURLToPath(new URL("../../src/lib/lock.ts", import.meta.url));

/** A pid that is certainly dead: a child that has exited. */
function deadPid(): Promise<number> {
  return new Promise((resolve) => {
    const c = spawn(process.execPath, ["-e", ""], { stdio: "ignore" });
    c.on("close", () => resolve(c.pid!));
  });
}

test("fn runs under the lock; the lock file holds {pid, at, purpose}; released afterwards", async () => {
  const path = join(tmpDir(), "sub", "a.lock");
  const seen = await withLock(path, "unit", () => JSON.parse(readFileSync(path, "utf8")));
  assert.equal(seen.pid, process.pid);
  assert.equal(seen.purpose, "unit");
  assert.ok(!Number.isNaN(Date.parse(seen.at)));
  assert.equal(existsSync(path), false);
});

test("the lock is released when fn throws", async () => {
  const path = join(tmpDir(), "a.lock");
  await assert.rejects(withLock(path, "unit", () => { throw new Error("boom"); }), /boom/);
  assert.equal(existsSync(path), false);
  assert.equal(await withLock(path, "again", () => 7), 7);
});

test("two processes holding the same lock serialise", async () => {
  const dir = tmpDir();
  const path = join(dir, "x.lock");
  const log = join(dir, "events.log");
  const code = `
    import { appendFileSync } from "node:fs";
    import { setTimeout as sleep } from "node:timers/promises";
    const { withLock } = await import(${JSON.stringify(LOCK_TS)});
    await withLock(${JSON.stringify(path)}, "child", async () => {
      appendFileSync(${JSON.stringify(log)}, "start " + process.pid + "\\n");
      await sleep(300);
      appendFileSync(${JSON.stringify(log)}, "end " + process.pid + "\\n");
    });`;
  const kids = [0, 1].map(
    () =>
      new Promise<number | null>((resolve) => {
        const c = spawn(process.execPath, ["--input-type=module", "-e", code], { stdio: ["ignore", "inherit", "inherit"] });
        c.on("close", resolve);
      }),
  );
  assert.deepEqual(await Promise.all(kids), [0, 0]);
  const events = readFileSync(log, "utf8").trim().split("\n");
  assert.equal(events.length, 4);
  // never start, start: each child's end follows its own start immediately
  for (let i = 0; i < 4; i += 2) {
    const [s, e] = [events[i]!.split(" "), events[i + 1]!.split(" ")];
    assert.deepEqual([s[0], e[0], s[1]], ["start", "end", e[1]]);
  }
});

test("a lock whose pid is dead is taken over, and logged to stderr", async () => {
  const path = join(tmpDir(), "a.lock");
  writeFileSync(path, JSON.stringify({ pid: await deadPid(), at: new Date().toISOString(), purpose: "crashed" }));
  const writes: string[] = [];
  const orig = process.stderr.write.bind(process.stderr);
  process.stderr.write = ((s: string) => (writes.push(String(s)), true)) as typeof process.stderr.write;
  try {
    assert.equal(await withLock(path, "unit", () => "got it", { timeoutMs: 2000 }), "got it");
  } finally {
    process.stderr.write = orig;
  }
  assert.ok(writes.some((w) => w.includes("stale lock") && w.includes("crashed")), writes.join());
});

test("a lock held by a live pid times out with exit 5 lock_timeout", async () => {
  const path = join(tmpDir(), "a.lock");
  writeFileSync(path, JSON.stringify({ pid: process.pid, at: new Date().toISOString(), purpose: "holder" }));
  await assert.rejects(
    withLock(path, "waiter", () => "never", { timeoutMs: 150 }),
    (e: any) => e.code === "lock_timeout" && e.exitCode === 5 && e.details.holder.purpose === "holder",
  );
  assert.equal(existsSync(path), true, "the holder's lock is left alone");
});

test("an empty lock file left behind long ago is treated as abandoned", async () => {
  const path = join(tmpDir(), "a.lock");
  writeFileSync(path, "");
  const old = new Date(Date.now() - 60_000);
  (await import("node:fs")).utimesSync(path, old, old);
  assert.equal(await withLock(path, "unit", () => "ok", { timeoutMs: 2000 }), "ok");
});

test("racing takeovers of one stale lock give each fn exclusive access", async () => {
  const dir = tmpDir();
  const path = join(dir, "a.lock");
  writeFileSync(path, JSON.stringify({ pid: await deadPid(), at: new Date().toISOString(), purpose: "crashed" }));
  let inside = 0;
  let maxInside = 0;
  const one = () =>
    withLock(path, "race", async () => {
      maxInside = Math.max(maxInside, ++inside);
      await new Promise((r) => setTimeout(r, 20));
      inside--;
    }, { timeoutMs: 5000 });
  await Promise.all([one(), one(), one(), one()]);
  assert.equal(maxInside, 1);
});

import { after, test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readLedger, withLedger } from "../../src/ledger/bd.ts";
import { BdIssues } from "../../src/ledger/schema.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { beadsDir, ledgerDir, ledgerLockPath } from "../../src/lib/paths.ts";
import { fakeBin } from "../helpers/index.ts";
import { addDraft, approvePlan, jobId, newPlan, sandbox } from "./helpers.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { z } from "zod";

const { repo } = sandbox();
const realPath = process.env.PATH;
after(() => {
  process.env.PATH = realPath;
});

const refusal = (code: string, exit: number) => (e: unknown) => e instanceof CpdError && e.code === code && e.exitCode === exit;

test("every bd call uses the §8 cwd and environment, --json, argv (no shell), and holds the ledger lock for writes", async () => {
  const wf = "fake";
  mkdirSync(ledgerDir(wf), { recursive: true });
  const lockSeen = join(ledgerDir(wf), "lock-seen");
  const bin = fakeBin(
    "bd",
    `pwd > '${join(ledgerDir(wf), "pwd.txt")}'
env > '${join(ledgerDir(wf), "env.txt")}'
[ -e '${ledgerLockPath(wf)}' ] && echo held >> '${lockSeen}'
echo '[]'`,
  );
  process.env.PATH = `${bin.dir}:${realPath}`;
  process.env.BEADS_ACTOR = "someone-else"; // must be overridden
  try {
    await withLedger(wf, "t", (l) => l.write(["update", "x; rm -rf /", "-d", "$(touch pwned)"]));
    await readLedger(wf).read(["list", "--all"], BdIssues);
  } finally {
    delete process.env.BEADS_ACTOR;
  }
  assert.deepEqual(bin.calls(), [["update", "x; rm -rf /", "-d", "$(touch pwned)", "--json"], ["list", "--all", "--json"]]);
  assert.equal(readFileSync(join(ledgerDir(wf), "pwd.txt"), "utf8").trim(), realpathSync(ledgerDir(wf)));
  const env = Object.fromEntries(readFileSync(join(ledgerDir(wf), "env.txt"), "utf8").split("\n").map((l) => l.split(/=(.*)/s).slice(0, 2)));
  assert.equal(env.BEADS_DIR, beadsDir(wf));
  assert.equal(env.BEADS_ACTOR, "cpd");
  assert.equal(env.BD_DISABLE_METRICS, "1");
  assert.equal(env.DO_NOT_TRACK, "1");
  assert.equal(env.BD_NON_INTERACTIVE, "1");
  assert.equal(readFileSync(lockSeen, "utf8"), "held\n", "the write ran under the lock, the read did not need it");
  assert.equal(existsSync(ledgerLockPath(wf)), false, "lock released");
  assert.equal(existsSync(join(ledgerDir(wf), "pwned")), false);
  process.env.PATH = realPath;
});

test("exit 13 is cas_lost and exit 14 is ledger_frozen (exit code first, stdout error object is the message)", async () => {
  const wf = "fakecodes";
  mkdirSync(ledgerDir(wf), { recursive: true });
  for (const [code, name] of [[13, "cas_lost"], [14, "ledger_frozen"]] as const) {
    const bin = fakeBin("bd", `echo '{"error":"boom ${code}"}'\nexit ${code}`);
    process.env.PATH = `${bin.dir}:${realPath}`;
    await assert.rejects(withLedger(wf, "t", (l) => l.write(["update", "x"])), (e: unknown) => {
      assert.ok(e instanceof CpdError);
      assert.equal(e.code, name);
      assert.equal(e.exitCode, 3);
      assert.match(e.message, new RegExp(`boom ${code}`));
      return true;
    });
  }
  const bin = fakeBin("bd", "echo oops >&2\nexit 1");
  process.env.PATH = `${bin.dir}:${realPath}`;
  await assert.rejects(withLedger(wf, "t", (l) => l.write(["update", "x"])), refusal("bd_failed", 1));
  const garbage = fakeBin("bd", "echo not json");
  process.env.PATH = `${garbage.dir}:${realPath}`;
  await assert.rejects(readLedger(wf).read(["list"], z.unknown()), refusal("bad_tool_output", 1));
  const wrong = fakeBin("bd", "echo '{\"a\":1}'");
  process.env.PATH = `${wrong.dir}:${realPath}`;
  await assert.rejects(readLedger(wf).read(["list"], BdIssues), refusal("bad_tool_output", 1));
  process.env.PATH = realPath;
});

test("the gateway refuses destructive commands and writes through a read handle", async () => {
  const wf = "fakeguard";
  mkdirSync(ledgerDir(wf), { recursive: true });
  const bin = fakeBin("bd", "echo '[]'");
  process.env.PATH = `${bin.dir}:${realPath}`;
  for (const c of ["delete", "prune", "flatten", "gc", "compact", "reopen", "admin"]) {
    await assert.rejects(withLedger(wf, "t", (l) => l.write([c, "x"])), refusal("bd_forbidden", 3), c);
  }
  for (const args of [["update", "x"], ["close", "x"], ["comments", "add", "x", "t"], ["create", "t"]]) {
    await assert.rejects(readLedger(wf).read(args, z.unknown()), refusal("bd_not_read_only", 1), args.join(" "));
  }
  assert.deepEqual(bin.calls(), []);
  process.env.PATH = realPath;
});

test("real bd: a lost compare-and-swap is cas_lost, and a frozen ledger is ledger_frozen while reads still work", async () => {
  const wf = "real";
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await approvePlan(wf);
  await lockPlan(wf);
  const id = await jobId(wf, "a");
  await assert.rejects(withLedger(wf, "t", (l) => l.write(["update", id, "--if-status", "running", "-s", "in_review"])), refusal("cas_lost", 3));
  const marker = join(ledgerDir(wf), "MIGRATION-FREEZE");
  writeFileSync(marker, "test\t2026-01-01T00:00:00Z\tfrozen\n");
  try {
    await assert.rejects(withLedger(wf, "t", (l) => l.write(["update", id, "-s", "running"])), refusal("ledger_frozen", 3));
    assert.equal((await readLedger(wf).read(["list", "--all", "-n", "0", "-t", "job"], BdIssues)).length, 1);
  } finally {
    rmSync(marker, { force: true });
  }
  await withLedger(wf, "t", (l) => l.write(["update", id, "-s", "running"]));
});

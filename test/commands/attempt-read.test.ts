import { test } from "node:test";
import assert from "node:assert/strict";
import { copyFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { transcriptLines } from "../../src/commands/attempt-read.ts";
import { recordAttemptFacts, startAttempt } from "../../src/ledger/attempts.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { tmpDir } from "../helpers/index.ts";
import { addDraft, approvePlan, cliSandbox, newPlan } from "../ledger/helpers.ts";

const FIXTURE = new URL("../herdr/fixtures/session-finished.jsonl", import.meta.url);
const dir = tmpDir("cpd-read-");

test("transcriptLines: assistant text lines and tool-call names in session order; users, tool results and thinking are left out", () => {
  const path = join(dir, "s.jsonl");
  copyFileSync(FIXTURE, path);
  assert.deepEqual(transcriptLines(path), ["tool: write", "assistant: Done: hello.txt created.", "assistant: Second block."]);
});

test("transcriptLines: a partial last line (a session being written) is ignored, a bad line elsewhere fails loud", () => {
  const good = JSON.stringify({ type: "message", message: { role: "assistant", content: [{ type: "text", text: "hi" }] } });
  const partial = join(dir, "partial.jsonl");
  writeFileSync(partial, `${good}\n{"type":"message","mess`);
  assert.deepEqual(transcriptLines(partial), ["assistant: hi"]);
  const broken = join(dir, "broken.jsonl");
  writeFileSync(broken, `{"nope\n${good}\n`);
  assert.throws(() => transcriptLines(broken), (e: unknown) => e instanceof CpdError && e.code === "session_malformed" && e.details.line === 1);
  assert.throws(() => transcriptLines(join(dir, "absent.jsonl")), (e: unknown) => e instanceof CpdError && e.code === "session_not_found" && e.exitCode === 4);
});

const { repo, cli } = cliSandbox();
const wf = "readcli";

test("cpd attempt read prints the last N lines of the Worker session recorded in the Ledger", async () => {
  await newPlan(wf, repo);
  await addDraft(wf, "api");
  await approvePlan(wf);
  await lockPlan(wf);
  await startAttempt(wf, "api", { tier: "S3" });
  const none = await cli(["attempt", "read", "api-a1", "--wf", wf]);
  assert.deepEqual([none.code, none.json.error.code], [4, "session_not_found"]);

  const session = join(dir, "worker.jsonl");
  copyFileSync(FIXTURE, session);
  await recordAttemptFacts(wf, "api-a1", { session_path: session });
  const all = await cli(["attempt", "read", "api-a1", "--wf", wf]);
  assert.equal(all.code, 0, all.stdout + all.stderr);
  assert.deepEqual(all.json.lines, ["tool: write", "assistant: Done: hello.txt created.", "assistant: Second block."]);
  assert.equal(all.json.session, session);
  const two = await cli(["attempt", "read", "api-a1", "--lines", "2", "--wf", wf]);
  assert.deepEqual(two.json.lines, ["assistant: Done: hello.txt created.", "assistant: Second block."]);
  assert.equal(two.json.total, 3);
});

test("attempt read: unknown Attempt is exit 4, bad --lines or id is exit 2", async () => {
  assert.equal((await cli(["attempt", "read", "api-a9", "--wf", wf])).code, 4);
  for (const lines of ["0", "-3", "x", "1.5"]) assert.equal((await cli(["attempt", "read", "api-a1", "--lines", lines, "--wf", wf])).code, 2, lines);
  assert.equal((await cli(["attempt", "read", "Bad", "--wf", wf])).code, 2);
  assert.equal((await cli(["attempt", "read", "--wf", wf])).code, 2);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { metricsSince } from "../../src/dispatch/attempt-run.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { tmpDir } from "../helpers/index.ts";

const dir = tmpDir("cpd-metrics-since-");

const assistant = (iso: string, input: number, stopReason = "stop") =>
  JSON.stringify({
    type: "message",
    timestamp: iso,
    message: { role: "assistant", content: [{ type: "text", text: "x" }], usage: { input, output: 1, cacheRead: 0, cacheWrite: 0, cost: { total: 0.01 } }, stopReason },
  });
const entry = (type: string, iso: string, extra: Record<string, unknown> = {}) => JSON.stringify({ type, timestamp: iso, ...extra });

/**
 * A forked Reviewer session as pi writes it (measured in the CPD_IT run): the header carries the fork time, and the
 * entries inherited from the Lead keep their ORIGINAL timestamps, so they are older than the header.
 */
function forked(): string {
  const path = join(dir, `fork-${Math.random().toString(36).slice(2)}.jsonl`);
  writeFileSync(
    path,
    [
      entry("session", "2026-01-01T10:05:00.000Z", { id: "r1", cwd: "/x", parentSession: "/lead.jsonl" }),
      entry("model_change", "2026-01-01T09:00:00.000Z", { modelId: "lead-model" }),
      assistant("2026-01-01T09:00:05.000Z", 1000), // the Lead's own turn, inherited
      assistant("2026-01-01T09:30:00.000Z", 2000), // another inherited Lead turn
      entry("model_change", "2026-01-01T10:05:01.000Z", { modelId: "reviewer-model" }),
      assistant("2026-01-01T10:05:10.000Z", 10, "toolUse"),
      assistant("2026-01-01T10:05:20.000Z", 20),
    ].join("\n") + "\n",
  );
  return path;
}

test("without a cut-off the whole file is counted (a Worker session)", () => {
  const m = metricsSince(forked());
  assert.equal(m.turns, 4);
  assert.equal(m.tokensIn, 3030);
});

test("with a cut-off only the Reviewer's own entries count, and the duration starts at the fork", () => {
  const m = metricsSince(forked(), Date.parse("2026-01-01T10:04:00.000Z"));
  assert.equal(m.turns, 2);
  assert.equal(m.tokensIn, 30);
  assert.equal(m.model, "reviewer-model");
  assert.equal(m.finalStopReason, "stop");
  assert.equal(m.durationS, 20);
});

test("the cut-off is inclusive, and a file that is all inherited history has zero turns", () => {
  assert.equal(metricsSince(forked(), Date.parse("2026-01-01T10:05:10.000Z")).turns, 2);
  assert.equal(metricsSince(forked(), Date.parse("2026-01-01T12:00:00.000Z")).turns, 0);
});

test("an entry that cannot be dated is kept, so a damaged file still fails loud", () => {
  const path = forked();
  appendFileSync(path, "{not json\n");
  assert.throws(() => metricsSince(path, Date.parse("2026-01-01T10:04:00.000Z")), (e: unknown) => e instanceof CpdError && e.code === "session_malformed");
});

test("the original file is never modified", () => {
  const path = forked();
  const before = readFileSync(path, "utf8");
  metricsSince(path, Date.parse("2026-01-01T10:04:00.000Z"));
  assert.equal(readFileSync(path, "utf8"), before);
});

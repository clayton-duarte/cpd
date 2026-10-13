import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { CpdError } from "../../src/lib/errors.ts";
import { sessionMetrics } from "../../src/metrics/session.ts";
import { tmpDir } from "../helpers/index.ts";

const fx = (n: string) => fileURLToPath(new URL(`./fixtures/${n}.jsonl`, import.meta.url));

// Independent computation over the same file (the JQ program below, run as `jq -s -c "$JQ" <file>`):
//   sums of .message.usage.{input,output,cacheRead,cacheWrite,cost.total} over assistant messages,
//   turns = count of assistant messages, durationS = last minus first entry timestamp (ms precision).
const JQ = `[.[]|select(.type=="message" and .message.role=="assistant")] as $a
 | def ms: (sub("\\\\.[0-9]+Z$";"Z")|fromdate) * 1000 + ((capture("\\\\.(?<f>[0-9]+)Z$").f // "0")|tonumber);
 {turns:($a|length), tokensIn:([$a[].message.usage.input]|add // 0), tokensOut:([$a[].message.usage.output]|add // 0),
  cacheRead:([$a[].message.usage.cacheRead]|add // 0), cacheWrite:([$a[].message.usage.cacheWrite]|add // 0),
  costUsd:([$a[].message.usage.cost.total]|add // 0), durationS:(((.[-1].timestamp|ms) - (.[0].timestamp|ms))/1000),
  finalStopReason:($a[-1].message.stopReason // null)}`;

function jqTotals(file: string) {
  const r = spawnSync("jq", ["-s", "-c", JQ, file], { encoding: "utf8" });
  assert.equal(r.status, 0, r.stderr);
  return JSON.parse(r.stdout) as Record<string, unknown>;
}

for (const name of ["finished", "interrupted"]) {
  test(`totals match jq: ${name}`, () => {
    const got = sessionMetrics(fx(name));
    const want = jqTotals(fx(name));
    for (const [k, v] of Object.entries(want)) {
      if (typeof v === "number") assert.ok(Math.abs((got as unknown as Record<string, number>)[k]! - v) < 1e-9, `${k}: ${got[k as keyof typeof got]} vs ${v}`);
      else assert.equal(got[k as keyof typeof got], v, k);
    }
    assert.equal(got.model, "claude-haiku-4.5");
    assert.equal(got.thinkingLevel, "low");
  });
}

test("final stop reasons: stop vs toolUse", () => {
  assert.equal(sessionMetrics(fx("finished")).finalStopReason, "stop");
  assert.equal(sessionMetrics(fx("interrupted")).finalStopReason, "toolUse");
});

test("skipped counts entries without metrics", () => {
  const m = sessionMetrics(fx("finished"));
  assert.ok(m.skipped > 0);
});

test("malformed JSON gives session_malformed with line number", () => {
  const dir = tmpDir();
  const header = '{"type":"session","timestamp":"2026-01-01T00:00:00.000Z"}';
  const p = join(dir, "bad.jsonl");
  writeFileSync(p, `${header}\n{"type":"custom","timestamp":"2026-01-01T00:00:01.000Z"}\n{not json\n`);
  assert.throws(
    () => sessionMetrics(p),
    (e: unknown) => e instanceof CpdError && e.code === "session_malformed" && e.details.line === 3,
  );
});

test("invalid usage on an assistant message gives session_malformed with line number", () => {
  const dir = tmpDir();
  const p = join(dir, "bad2.jsonl");
  writeFileSync(
    p,
    '{"type":"session","timestamp":"2026-01-01T00:00:00.000Z"}\n{"type":"message","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","stopReason":"stop"}}\n',
  );
  assert.throws(
    () => sessionMetrics(p),
    (e: unknown) => e instanceof CpdError && e.code === "session_malformed" && e.details.line === 2,
  );
});

test("header-only session gives zeros and null stop reason", () => {
  const dir = tmpDir();
  const p = join(dir, "empty.jsonl");
  writeFileSync(p, '{"type":"session","version":3,"id":"x","timestamp":"2026-01-01T00:00:00.000Z"}\n');
  assert.deepEqual(sessionMetrics(p), {
    turns: 0, tokensIn: 0, tokensOut: 0, cacheRead: 0, cacheWrite: 0, costUsd: 0,
    durationS: 0, model: null, thinkingLevel: null, finalStopReason: null, skipped: 0,
  });
});

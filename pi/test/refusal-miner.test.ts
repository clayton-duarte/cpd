import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { mineRefusals } from "../lib/refusal-miner.ts";

// The miner over fixture session files (pi/tools/refusal-miner/README.md, contract §5.7). The
// fixtures hold only synthetic sessions; they are never a real session file of the User or an agent.

const FIXTURES = new URL("./refusal-miner/fixtures/", import.meta.url);
const SINCE = "2026-11-07T00:00:00.000Z";

const text = readFileSync(new URL("sessions.jsonl", FIXTURES), "utf8");
const expected = JSON.parse(readFileSync(new URL("expected.json", FIXTURES), "utf8")) as {
  total: number;
  guards: Record<string, number>;
  byRole: { guard: string; role: string; count: number }[];
  byModel: { guard: string; model: string; count: number }[];
  absent: string[];
};

const mined = mineRefusals([{ name: "sessions.jsonl", text }], { since: SINCE });

test("counts refusals per guard and code", () => {
  const counts = Object.fromEntries(mined.byGuard.map((r) => [r.guard, r.count]));
  assert.deepEqual(counts, expected.guards);
  assert.equal(mined.total, expected.total);
  for (const r of mined.byGuard) {
    assert.ok(typeof r.sample === "string" && r.sample.length > 0, `${r.guard}: sample is the exact matched string`);
    if (r.guard.startsWith("φ: ")) {
      assert.ok(r.sample.startsWith("φ: "), `${r.guard}: sample is the refusal prefix (${r.sample})`);
    } else {
      const code = r.guard.slice("cli: ".length);
      assert.ok(r.sample.includes(`"code":"${code}"`), `${r.guard}: sample quotes the envelope's code (${r.sample})`);
    }
  }
});

test("breaks refusals down by role and model", () => {
  assert.deepEqual(mined.byRole, expected.byRole);
  assert.deepEqual(mined.byModel, expected.byModel);
});

test("ignores quoted refusal text", () => {
  const guards = new Set(mined.byGuard.map((r) => r.guard));
  for (const g of expected.absent) assert.ok(!guards.has(g), `${g} is only quoted and is not counted`);
  assert.ok(guards.has("cli: usage"), "a real refusal is still counted");
  assert.equal(mined.total, expected.total, "the quoted envelopes do not inflate the total");
});

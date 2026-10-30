import assert from "node:assert/strict";
import { mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { CpdError } from "../../lib/cpd.ts";
import { endReport, writeReport, type EndReport, type Entry } from "../../durable-worker/report.ts";

type Over = { stopReason?: string; errorMessage?: string; provider?: string; model?: string; input?: number; output?: number; cacheRead?: number; cacheWrite?: number; total?: number };

const assistant = (o: Over = {}): Entry => ({
  kind: "pi.assistant",
  model: [
    {
      role: "assistant",
      stopReason: o.stopReason ?? "stop",
      ...(o.errorMessage === undefined ? {} : { errorMessage: o.errorMessage }),
      provider: o.provider ?? "anthropic",
      model: o.model ?? "sonnet",
      usage: { input: o.input ?? 0, output: o.output ?? 0, cacheRead: o.cacheRead ?? 0, cacheWrite: o.cacheWrite ?? 0, cost: { total: o.total ?? 0 } },
    },
  ],
});
const other: Entry = { kind: "pi.user", model: [{ role: "user" }] };

test("endReport takes the stop reason and error message of the last assistant entry", () => {
  const r = endReport([assistant({ stopReason: "toolUse" }), other, assistant({ stopReason: "error", errorMessage: "boom" })]);
  assert.equal(r.stop_reason, "error");
  assert.equal(r.error_message, "boom");
  const ok = endReport([assistant({ stopReason: "error", errorMessage: "old" }), assistant({ stopReason: "stop" })]);
  assert.equal(ok.stop_reason, "stop");
  assert.equal(ok.error_message, null);
});

test("endReport counts assistant entries as turns and sums usage", () => {
  const r = endReport([
    assistant({ input: 10, output: 5, cacheRead: 1, cacheWrite: 2, total: 0.5 }),
    other,
    assistant({ input: 20, output: 7, cacheRead: 3, cacheWrite: 4, total: 0.25 }),
  ]);
  assert.equal(r.turns, 2);
  assert.equal(r.tokens_in, 30);
  assert.equal(r.tokens_out, 12);
  assert.equal(r.cache_read, 4);
  assert.equal(r.cache_write, 6);
  assert.equal(r.cost_usd, 0.75);
});

test("endReport names the model of the last assistant entry", () => {
  const r = endReport([assistant({ provider: "openai", model: "gpt" }), other, assistant({ provider: "anthropic", model: "opus" })]);
  assert.equal(r.model, "anthropic/opus");
});

test("endReport refuses entries with no assistant entry", () => {
  for (const entries of [[], [other]]) {
    try {
      endReport(entries);
    } catch (e) {
      if (!(e instanceof CpdError)) throw e;
      assert.equal(e.code, "no_assistant_entry");
      continue;
    }
    assert.fail("did not throw");
  }
});

const roots: string[] = [];
after(async () => {
  await Promise.all(roots.map((r) => rm(r, { recursive: true, force: true })));
});

test("writeReport creates the missing directory, replaces an earlier report and leaves no temp file", async () => {
  const root = await mkdtemp(join(tmpdir(), "report-"));
  roots.push(root);
  const dir = join(root, "run", "plan", "attempts");
  const file = join(dir, "a1.report.json");
  const first = endReport([assistant({ input: 1 })]);
  await writeReport(file, first);
  assert.deepEqual(JSON.parse(await readFile(file, "utf8")) as EndReport, first);
  const second = endReport([assistant({ input: 1 }), assistant({ input: 2 })]);
  await writeReport(file, second);
  assert.deepEqual(JSON.parse(await readFile(file, "utf8")) as EndReport, second);
  assert.deepEqual(await readdir(dir), ["a1.report.json"]);
});

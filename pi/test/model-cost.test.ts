import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { report } from "../measure/model-cost.ts";

const line = (o: unknown) => JSON.stringify(o);
const header = (timestamp = "2026-10-30T10:00:00.000Z") =>
  line({ type: "session", version: 3, id: "s", timestamp, cwd: "/x" });
const info = (name: string) => line({ type: "session_info", name });
const usage = (input: number, output: number, cacheRead: number, cacheWrite: number, cost: number) => ({
  input,
  output,
  cacheRead,
  cacheWrite,
  cost: { total: cost },
});
const assistantTurn = (
  id: string,
  model: string,
  u: { input: number; output: number; cacheRead: number; cacheWrite: number; cost: { total: number } },
  timestamp = "2026-10-30T14:00:00.000Z",
  content: unknown[] = [{ type: "text", text: "hello" }],
) =>
  line({
    type: "message",
    id,
    timestamp,
    message: {
      role: "assistant",
      model,
      content,
      usage: u,
    },
  });

function withTempDir<T>(fn: (dir: string) => T): T {
  const dir = mkdtempSync(join(tmpdir(), "model-cost-test-"));
  try {
    return fn(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("aggregates spend by role model and day", () => {
  withTempDir((tmp) => {
    const root = join(tmp, "sessions");
    const sub = join(root, "sub");
    mkdirSync(sub, { recursive: true });

    // Worker session (worker attempt format name: e.g. "plan job a1")
    // Contains two turns across two days (Toronto time: UTC 14:00 is same day)
    // 2026-10-30T14:00:00Z -> 2026-10-30 (Toronto)
    // 2026-10-31T14:00:00Z -> 2026-10-31 (Toronto)
    const workerJsonl = [
      header("2026-10-30T10:00:00.000Z"),
      info("model-cost measure a1"),
      assistantTurn("w1", "vertex/claude-sonnet-4-latest", usage(100, 50, 1000, 200, 0.05), "2026-10-30T14:00:00.000Z"),
      assistantTurn("w2", "vertex/claude-sonnet-4-latest", usage(150, 60, 2000, 100, 0.08), "2026-10-31T14:00:00.000Z"),
      // Zero usage turn (abort/error) should be counted in zero_usage_turns and excluded from totals
      assistantTurn("w3", "vertex/claude-sonnet-4-latest", usage(0, 0, 0, 0, 0), "2026-10-31T15:00:00.000Z"),
      // Unparseable non-JSON line to test skipped_lines
      "this is not valid json",
    ].join("\n");
    writeFileSync(join(sub, "worker.jsonl"), workerJsonl);

    // Manager session (ends with -manager)
    const managerJsonl = [
      header("2026-10-30T10:00:00.000Z"),
      info("ctx-manager"),
      assistantTurn("m1", "anthropic/claude-haiku-5-5", usage(200, 40, 500, 50, 0.02), "2026-10-30T16:00:00.000Z"),
    ].join("\n");
    writeFileSync(join(sub, "manager.jsonl"), managerJsonl);

    // Lead session (other name)
    const leadJsonl = [
      header("2026-10-30T10:00:00.000Z"),
      info("lead-orchestrator"),
      assistantTurn("l1", "vertex/claude-sonnet-4-latest", usage(300, 80, 1500, 300, 0.10), "2026-10-30T18:00:00.000Z"),
    ].join("\n");
    writeFileSync(join(sub, "lead.jsonl"), leadJsonl);

    // Other session (no session_info)
    const otherJsonl = [
      header("2026-10-30T10:00:00.000Z"),
      assistantTurn("o1", "anthropic/claude-haiku-5-5", usage(50, 20, 200, 10, 0.01), "2026-10-30T20:00:00.000Z"),
    ].join("\n");
    writeFileSync(join(sub, "other.jsonl"), otherJsonl);

    const emptyDurable = join(tmp, "durable");
    mkdirSync(emptyDurable, { recursive: true });

    const r = report({
      root,
      durableRoot: emptyDurable,
    });

    assert.equal(r.files.pi, 4);
    assert.equal(r.files.durable, 0);
    assert.equal(r.skipped_lines, 1);
    assert.equal(r.zero_usage_turns, 1);

    // Totals:
    // Turns: w1 (100,50,1000,200), w2 (150,60,2000,100), m1 (200,40,500,50), l1 (300,80,1500,300), o1 (50,20,200,10)
    // Turns count: 5
    // Input: 100 + 150 + 200 + 300 + 50 = 800
    // Output: 50 + 60 + 40 + 80 + 20 = 250
    // CacheRead: 1000 + 2000 + 500 + 1500 + 200 = 5200
    // CacheWrite: 200 + 100 + 50 + 300 + 10 = 660
    // Prompt: input + cache_read + cache_write = 800 + 5200 + 660 = 6660
    // Hit rate: 5200 / 6660
    // Cost: 0.05 + 0.08 + 0.02 + 0.10 + 0.01 = 0.26
    assert.equal(r.totals.turns, 5);
    assert.equal(r.totals.input, 800);
    assert.equal(r.totals.output, 250);
    assert.equal(r.totals.cache_read, 5200);
    assert.equal(r.totals.cache_write, 660);
    assert.equal(r.totals.prompt_tokens, 6660);
    assert.ok(Math.abs((r.totals.cache_hit_rate ?? 0) - 5200 / 6660) < 1e-6);
    assert.ok(Math.abs(r.totals.cost_usd - 0.26) < 1e-6);

    // Role breakdowns
    assert.equal(r.by_role.worker.turns, 2);
    assert.equal(r.by_role.worker.cache_read, 3000);
    assert.ok(Math.abs(r.by_role.worker.cost_usd - 0.13) < 1e-6);

    assert.equal(r.by_role.manager.turns, 1);
    assert.equal(r.by_role.manager.cache_read, 500);

    assert.equal(r.by_role.lead.turns, 1);
    assert.equal(r.by_role.lead.cache_read, 1500);

    assert.equal(r.by_role.other.turns, 1);
    assert.equal(r.by_role.other.cache_read, 200);

    // By model
    assert.ok("vertex/claude-sonnet-4-latest" in r.by_model);
    assert.ok("anthropic/claude-haiku-5-5" in r.by_model);
    assert.equal(r.by_model["vertex/claude-sonnet-4-latest"]?.turns, 3);
    assert.equal(r.by_model["anthropic/claude-haiku-5-5"]?.turns, 2);

    // By day (calendar day in America/Toronto)
    // 2026-10-30: w1, m1, l1, o1 (4 turns)
    // 2026-10-31: w2 (1 turn)
    assert.ok("2026-10-30" in r.by_day);
    assert.ok("2026-10-31" in r.by_day);
    assert.equal(r.by_day["2026-10-30"]?.turns, 4);
    assert.equal(r.by_day["2026-10-31"]?.turns, 1);

    // Filtering by window (--since, --until applied to assistant turn timestamps)
    const filtered = report({
      root,
      durableRoot: emptyDurable,
      since: "2026-10-31T00:00:00.000Z",
      until: "2026-11-01T00:00:00.000Z",
    });
    // Only w2 falls in this window
    assert.equal(filtered.totals.turns, 1);
    assert.equal(filtered.totals.input, 150);
  });
});

test("detects cache break events and first changed token range", () => {
  withTempDir((tmp) => {
    const root = join(tmp, "sessions");
    const sub = join(root, "sub");
    mkdirSync(sub, { recursive: true });

    // A session with cache break:
    // Turn 1: cacheRead = 6000 (>= 5000 MIN_PREV_CACHE_READ) at T0
    // Turn 2: cacheRead = 2000 (< 50% BREAK_RATIO of 6000) at T0 + 100s -> partial break!
    // Event between turn 1 and turn 2: compaction -> cause = "compaction"
    // Turn 3: cacheRead = 7000 at T0 + 200s
    // Turn 4: cacheRead = 0 at T0 + 600s (> 300s idle gap) -> to_zero break, idle_gap = true, cause = "none_logged"
    const sessionLines = [
      header("2026-10-30T10:00:00.000Z"),
      info("worker-session a1"),
      line({
        type: "message",
        id: "m0",
        message: { role: "user", content: [{ type: "text", text: "prompt prefix AAA" }] },
      }),
      assistantTurn("a1", "bifrost/vertex/claude-sonnet-5", usage(100, 50, 6000, 200, 0.05), "2026-10-30T10:01:00.000Z"),
      // Compaction event between turns
      line({ type: "compaction", timestamp: "2026-10-30T10:01:30.000Z" }),
      line({
        type: "message",
        id: "m1",
        message: { role: "user", content: [{ type: "text", text: "prompt prefix BBB with changes" }] },
      }),
      assistantTurn("a2", "bifrost/vertex/claude-sonnet-5", usage(100, 50, 2000, 200, 0.05), "2026-10-30T10:02:40.000Z"),
      assistantTurn("a3", "bifrost/vertex/claude-sonnet-5", usage(100, 50, 7000, 200, 0.05), "2026-10-30T10:04:00.000Z"),
      assistantTurn("a4", "bifrost/vertex/claude-sonnet-5", usage(100, 50, 0, 200, 0.05), "2026-10-30T10:10:00.000Z"),
    ];
    writeFileSync(join(sub, "session.jsonl"), sessionLines.join("\n"));

    const emptyDurable = join(tmp, "durable");
    mkdirSync(emptyDurable, { recursive: true });

    const r = report({
      root,
      durableRoot: emptyDurable,
      diffPrefix: true,
    });

    assert.equal(r.cache_breaks.length, 2);

    // Break 1: a1 -> a2
    const b1 = r.cache_breaks[0]!;
    assert.equal(b1.class, "partial");
    assert.equal(b1.prev_cache_read, 6000);
    assert.equal(b1.cache_read, 2000);
    assert.equal(b1.gap_s, 100);
    assert.equal(b1.idle_gap, false);
    assert.equal(b1.cause, "compaction");
    assert.equal(b1.model, "bifrost/vertex/claude-sonnet-5");
    assert.ok(b1.diff !== undefined);
    assert.equal(b1.diff.scope, "messages");
    assert.equal(b1.diff.prefix_changed, true);
    assert.ok(b1.diff.tokens_from > 0);
    assert.ok(b1.diff.cur_tokens_to >= b1.diff.tokens_from);

    // Break 2: a3 -> a4
    const b2 = r.cache_breaks[1]!;
    assert.equal(b2.class, "to_zero");
    assert.equal(b2.prev_cache_read, 7000);
    assert.equal(b2.cache_read, 0);
    assert.equal(b2.gap_s, 360);
    assert.equal(b2.idle_gap, true);
    assert.equal(b2.cause, "none_logged");
  });
});

test("parses durable worker sqlite sessions and reconciles with bifrost csv", () => {
  withTempDir((tmp) => {
    const emptyPi = join(tmp, "sessions");
    mkdirSync(emptyPi, { recursive: true });

    const durableRoot = join(tmp, "durable_run");
    const planDurable = join(durableRoot, "plan1", "durable");
    mkdirSync(planDurable, { recursive: true });

    // Create a mock durable worker sqlite database
    const dbPath = join(planDurable, "worker-job-a1.sqlite");
    const db = new DatabaseSync(dbPath);
    try {
      db.exec(`
        CREATE TABLE entries (
          id INTEGER PRIMARY KEY,
          conversation_id INTEGER NOT NULL,
          head INTEGER,
          commit_seq INTEGER NOT NULL,
          record TEXT NOT NULL
        ) STRICT;
      `);

      // Turn 1 on UTC 2026-10-30: 10:00:00Z = 1793354400000 ms
      // Model: bifrost/vertex/claude-opus-5-5
      const ts1 = Date.parse("2026-10-30T10:00:00.000Z");
      const record1 = JSON.stringify({
        kind: "pi.assistant",
        id: 1,
        conversationId: 101,
        model: [
          {
            role: "assistant",
            provider: "bifrost",
            model: "vertex/claude-opus-5-5",
            usage: {
              input: 1000,
              output: 200,
              cacheRead: 50000,
              cacheWrite: 2000,
              cost: { total: 10.50 },
            },
            timestamp: ts1,
          },
        ],
      });

      // Turn 2 on UTC 2026-10-30: 12:00:00Z
      const ts2 = Date.parse("2026-10-30T12:00:00.000Z");
      const record2 = JSON.stringify({
        kind: "pi.assistant",
        id: 2,
        conversationId: 101,
        model: [
          {
            role: "assistant",
            provider: "bifrost",
            model: "vertex/claude-opus-5-5",
            usage: {
              input: 2000,
              output: 300,
              cacheRead: 30000,
              cacheWrite: 1000,
              cost: { total: 15.25 },
            },
            timestamp: ts2,
          },
        ],
      });

      db.prepare("INSERT INTO entries (id, conversation_id, head, commit_seq, record) VALUES (?, ?, ?, ?, ?)").run(
        1, 101, null, 1, record1,
      );
      db.prepare("INSERT INTO entries (id, conversation_id, head, commit_seq, record) VALUES (?, ?, ?, ?, ?)").run(
        2, 101, null, 2, record2,
      );
    } finally {
      db.close();
    }

    // Create a mock Bifrost CSV export
    // The CSV has # overview-tokens and # overview-cost sections
    // Model stripped matches: bifrost/vertex/claude-opus-5-5 -> claude-opus-5-5
    const csvContent = [
      "# overview-volume",
      '"Timestamp","Total Requests","Success"',
      '"2026-10-30T00:00:00Z","100","100"',
      "",
      "# overview-tokens",
      '"Timestamp","Prompt Tokens","Completion Tokens","Total Tokens","Cached Read Tokens"',
      '"2026-10-30T00:00:00Z","100000","5000","105000","80000"',
      "",
      "# overview-cost",
      '"Timestamp","Total Cost","claude-opus-5-5"',
      '"2026-10-30T00:00:00Z","30.00","30.00"',
      "",
    ].join("\n");
    const csvPath = join(tmp, "bifrost.csv");
    writeFileSync(csvPath, csvContent);

    const r = report({
      root: emptyPi,
      durableRoot,
      bifrostCsv: csvPath,
    });

    // Durable parsing check:
    assert.equal(r.files.durable, 1);
    assert.equal(r.files.bifrost, 1);
    // Durable sessions are all 'worker' role
    assert.equal(r.by_role.worker.turns, 2);
    assert.equal(r.by_role.worker.cache_read, 80000);
    assert.ok(Math.abs(r.by_role.worker.cost_usd - 25.75) < 1e-6);

    // Reconciliation check:
    // UTC day 2026-10-30, model claude-opus-5-5
    // Local cost: 10.50 + 15.25 = 25.75
    // Bifrost cost: 30.00
    // Delta = Bifrost - Local = 30.00 - 25.75 = 4.25
    // Ratio = Local / Bifrost = 25.75 / 30.00
    // Local cached read: 80000
    // Bifrost cached read: 80000
    assert.equal(r.reconciliation.length, 1);
    const row = r.reconciliation[0]!;
    assert.equal(row.utc_day, "2026-10-30");
    assert.equal(row.model, "claude-opus-5-5");
    assert.ok(Math.abs(row.local_cost_usd - 25.75) < 1e-6);
    assert.ok(Math.abs(row.bifrost_cost_usd - 30.00) < 1e-6);
    assert.ok(Math.abs(row.delta_usd - 4.25) < 1e-6);
    assert.ok(Math.abs((row.ratio ?? 0) - 25.75 / 30.00) < 1e-6);
    assert.equal(row.local_cache_read, 80000);
    assert.equal(row.bifrost_cached_read, 80000);
  });
});

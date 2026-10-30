import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { main, report } from "../measure/session-bloat.ts";

const line = (o: unknown) => JSON.stringify(o);
const header = (timestamp = "2026-10-30T10:00:00.000Z") => line({ type: "session", version: 3, id: "s", timestamp, cwd: "/x" });
const info = (name: string) => line({ type: "session_info", name });
const usage = (input: number, output: number, cacheRead: number, cacheWrite: number, cost: number) => ({
  input,
  output,
  cacheRead,
  cacheWrite,
  cost: { total: cost },
});
const assistant = (id: string, callId: string, name: string, args: unknown, u = usage(1, 1, 0, 0, 0)) =>
  line({
    type: "message",
    id,
    message: { role: "assistant", content: [{ type: "toolCall", id: callId, name, arguments: args }], usage: u },
  });
const result = (id: string, callId: string, toolName: string, text: string) =>
  line({
    type: "message",
    id,
    message: { role: "toolResult", toolCallId: callId, toolName, content: [{ type: "text", text }], isError: false },
  });
const codemodeResult = (id: string, callId: string, text: string, names: string[]) =>
  line({
    type: "message",
    id,
    message: {
      role: "toolResult",
      toolCallId: callId,
      toolName: "codemode",
      content: [{ type: "text", text }],
      isError: false,
      nestedCalls: { calls: names.map((name) => ({ name, status: "ok" })), complete: true },
    },
  });
const file = (name: string, ...lines: string[]) => ({ name, text: lines.join("\n") + "\n" });

test("a session named like a Worker attempt is a worker and one ending in manager is a manager", () => {
  const r = report(
    [
      file("w.jsonl", header(), info("plan job a2")),
      file("m.jsonl", header(), info("ctx-manager")),
      file("l.jsonl", header(), info("my lead")),
      file("o.jsonl", header()),
    ],
    {},
  );
  assert.deepEqual(
    [r.roles.worker.sessions, r.roles.manager.sessions, r.roles.lead.sessions, r.roles.other.sessions],
    [1, 1, 1, 1],
  );
  assert.equal(r.files, 4);
});

test("tool-result characters are summed per tool name", () => {
  const r = report(
    [
      file(
        "w.jsonl",
        header(),
        info("p j a1"),
        assistant("a1", "c1", "read", { path: "x" }),
        result("r1", "c1", "read", "x".repeat(10)),
        assistant("a2", "c2", "read", { path: "y" }),
        result("r2", "c2", "read", "y".repeat(30)),
        assistant("a3", "c3", "grep", {}),
        result("r3", "c3", "grep", "z".repeat(50)),
      ),
    ],
    {},
  );
  const w = r.roles.worker;
  assert.equal(w.tool_results, 3);
  assert.equal(w.result_chars, 90);
  assert.equal(w.result_tokens_est, 23);
  assert.deepEqual(w.by_tool, [
    { tool: "grep", calls: 1, chars: 50 },
    { tool: "read", calls: 2, chars: 40 },
  ]);
});

test("a bash result is keyed by the first two words of its command", () => {
  const r = report(
    [
      file(
        "w.jsonl",
        header(),
        info("p j a1"),
        assistant("a1", "c1", "bash", { command: "gh run view 123 --log-failed" }),
        result("r1", "c1", "bash", "a".repeat(100)),
        result("r2", "missing", "bash", "b".repeat(5)),
        assistant("a2", "c2", "read", {}),
        result("r3", "c2", "read", "c".repeat(7)),
      ),
    ],
    {},
  );
  assert.deepEqual(r.roles.worker.top_commands, [
    { key: "gh run", calls: 1, chars: 100 },
    { key: "read", calls: 1, chars: 7 },
    { key: "bash ?", calls: 1, chars: 5 },
  ]);
  assert.deepEqual(r.roles.worker.largest[0], {
    chars: 100,
    tool: "bash",
    command: "gh run view 123 --log-failed",
    file: "w.jsonl",
    entry: "r1",
  });
});

test("a codemode result counts its own output and its nested calls by name", () => {
  const r = report(
    [
      file(
        "w.jsonl",
        header(),
        info("p j a1"),
        assistant("a1", "c1", "codemode", {}),
        codemodeResult("r1", "c1", "x".repeat(12), ["bash", "read", "bash"]),
        assistant("a2", "c2", "codemode", {}),
        codemodeResult("r2", "c2", "y".repeat(8), ["bash"]),
      ),
    ],
    {},
  );
  assert.deepEqual(r.roles.worker.codemode, {
    calls: 2,
    output_chars: 20,
    nested: [
      { name: "bash", calls: 3 },
      { name: "read", calls: 1 },
    ],
  });
});

test("assistant usage is summed per role", () => {
  const r = report(
    [
      file(
        "w.jsonl",
        header(),
        info("p j a1"),
        assistant("a1", "c1", "read", {}, usage(10, 2, 100, 5, 0.5)),
        assistant("a2", "c2", "read", {}, usage(20, 3, 200, 6, 0.25)),
      ),
      file("m.jsonl", header(), info("ctx-manager"), assistant("a1", "c1", "read", {}, usage(7, 1, 0, 0, 1))),
    ],
    {},
  );
  assert.equal(r.roles.worker.turns, 2);
  assert.deepEqual(r.roles.worker.usage, { input: 30, output: 5, cache_read: 300, cache_write: 11, cost_usd: 0.75 });
  assert.deepEqual(r.roles.manager.usage, { input: 7, output: 1, cache_read: 0, cache_write: 0, cost_usd: 1 });
});

test("sessions outside the since and until window are left out", () => {
  const f = (n: string, ts: string) => file(n, header(ts), info("p j a1"));
  const files = [
    f("a.jsonl", "2026-10-01T00:00:00.000Z"),
    f("b.jsonl", "2026-10-10T00:00:00.000Z"),
    f("c.jsonl", "2026-10-20T00:00:00.000Z"),
  ];
  const r = report(files, { since: "2026-10-10T00:00:00.000Z", until: "2026-10-20T00:00:00.000Z" });
  assert.equal(r.roles.worker.sessions, 1);
  assert.deepEqual(r.window, { since: "2026-10-10T00:00:00.000Z", until: "2026-10-20T00:00:00.000Z" });
  assert.equal(report(files, {}).window.since, null);
});

test("a line that is not JSON is counted in the report and not hidden", () => {
  const r = report(
    [
      {
        name: "w.jsonl",
        text: [header(), info("p j a1"), assistant("a1", "c1", "read", {}), '{"type":"message","id":"a2","mess'].join("\n"),
      },
    ],
    {},
  );
  assert.equal(r.skipped_lines, 1);
  assert.equal(r.roles.worker.turns, 1);
});

test("main prints the JSON report for a directory of sessions and writes nothing", (t) => {
  const root = mkdtempSync(join(tmpdir(), "bloat-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "--x--"));
  const path = join(root, "--x--", "1_s.jsonl");
  writeFileSync(
    path,
    [header(), info("p j a1"), assistant("a1", "c1", "read", {}), result("r1", "c1", "read", "abcd")].join("\n") + "\n",
  );
  const before = { bytes: readFileSync(path), mtime: statSync(path).mtimeMs };
  const out: string[] = [];
  const err: string[] = [];
  const io = { out: (s: string) => void out.push(s), err: (s: string) => void err.push(s), home: root };
  assert.equal(main(["--root", root, "--json"], io), 0);
  assert.equal(out.length, 1);
  const parsed = JSON.parse(out[0]!);
  assert.equal(parsed.files, 1);
  assert.equal(parsed.roles.worker.result_chars, 4);
  assert.deepEqual(readFileSync(path), before.bytes);
  assert.equal(statSync(path).mtimeMs, before.mtime);

  const out2: string[] = [];
  const err2: string[] = [];
  const io2 = { out: (s: string) => void out2.push(s), err: (s: string) => void err2.push(s), home: root };
  assert.equal(main(["--root", root, "--since", "not-a-time"], io2), 2);
  assert.equal(err2.length, 1);
  assert.equal(out2.length, 0);
  assert.equal(err.length, 0);
});

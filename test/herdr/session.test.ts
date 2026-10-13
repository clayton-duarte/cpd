import { test } from "node:test";
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { lastAssistant, readSessionHeader } from "../../src/herdr/session.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { tmpDir } from "../helpers/index.ts";

const fx = (name: string) => new URL(`./fixtures/${name}`, import.meta.url).pathname;
const rejects = (fn: () => unknown, code: string, exitCode = 1) =>
  assert.throws(fn, (e: unknown) => e instanceof CpdError && e.code === code && e.exitCode === exitCode, code);

test("readSessionHeader returns id and cwd; parentSession only for a fork", () => {
  assert.deepEqual(readSessionHeader(fx("session-finished.jsonl")), {
    id: "01a132ad-8b94-7785-9c0b-e72213509328",
    cwd: "/private/tmp/wt1",
  });
  assert.deepEqual(readSessionHeader(fx("session-fork.jsonl")), {
    id: "01a132ae-0000-7000-8000-000000000001",
    cwd: "/private/tmp/lead",
    parentSession: "/home/u/.pi/agent/sessions/--lead--/lead.jsonl",
  });
});

test("readSessionHeader: a missing file is session_not_found (exit 4); a non-header first line is session_malformed", () => {
  rejects(() => readSessionHeader("/nonexistent/s.jsonl"), "session_not_found", 4);
  const f = join(tmpDir(), "s.jsonl");
  writeFileSync(f, '{"type":"message"}\n');
  rejects(() => readSessionHeader(f), "session_malformed");
  writeFileSync(f, "");
  rejects(() => readSessionHeader(f), "session_malformed");
});

test("lastAssistant: a finished session ends with stopReason stop and its text blocks", () => {
  assert.deepEqual(lastAssistant(fx("session-finished.jsonl")), {
    stopReason: "stop",
    text: "Done: hello.txt created.\nSecond block.",
  });
});

test("lastAssistant: a killed session (process died mid-turn) ends with toolUse and no text", () => {
  assert.deepEqual(lastAssistant(fx("session-killed.jsonl")), { stopReason: "toolUse", text: "" });
});

test("lastAssistant: null while there is no assistant message; unknown entry types are ignored", () => {
  assert.equal(lastAssistant(fx("session-noassistant.jsonl")), null);
});

test("lastAssistant: malformed lines fail loud with the line number", () => {
  const f = join(tmpDir(), "s.jsonl");
  const good = '{"type":"session","id":"x","cwd":"/c"}';
  for (const [bad, why] of [
    ["{not json", "invalid JSON"],
    ["[1]", "not a session entry"],
    ['{"type":"message","message":{"content":"x"}}', "without a role"],
    ['{"type":"message","message":{"role":"assistant","content":[]}}', "bad assistant message"],
  ] as const) {
    writeFileSync(f, `${good}\n${bad}\n`);
    assert.throws(
      () => lastAssistant(f),
      (e: unknown) => e instanceof CpdError && e.code === "session_malformed" && e.details.line === 2 && e.message.includes(why),
      bad,
    );
  }
  writeFileSync(f, `${good}\n\n${good}\n`);
  rejects(() => lastAssistant(f), "session_malformed");
});

test("lastAssistant: string content is accepted", () => {
  const f = join(tmpDir(), "s.jsonl");
  writeFileSync(f, '{"type":"message","message":{"role":"assistant","content":"plain","stopReason":"stop"}}\n');
  assert.deepEqual(lastAssistant(f), { stopReason: "stop", text: "plain" });
});

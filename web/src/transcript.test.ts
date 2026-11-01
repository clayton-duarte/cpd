// Transcript-parser tests for the dev live adapter (web/README.md "The TypeScript contract",
// hs-d819). RED: `parsePiSession`, `parseTailOutput` and `toSnapshotFrame` still throw
// `not implemented`, so these fail now and pass once the impl lands.
//
// The pi input is a sanitized excerpt of a real pi session file under ~/.pi/agent/sessions (the
// fixture's first line is its `session` header, the rest real `message` lines); the tail input
// is derived from docs/contract.md §6.20 (no durable Session was live). The fixture header says
// so. Only authored code is tested: the parsers.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { parsePiSession, parseTailOutput, toSnapshotFrame, type TranscriptMessage } from "./transcript";

/** Reads a fixture committed beside this test. */
function fixture(name: string): string {
  return readFileSync(new URL(name, import.meta.url), "utf8");
}

const piJsonl = (): string => fixture("./api/pi-session.fixture.jsonl");
const tailOutput = (): string => fixture("./api/tail.fixture.txt");

test("parsePiSession reads a real pi session JSONL into user and assistant messages", () => {
  const messages = parsePiSession(piJsonl());
  // Only the two roles are kept, one message per line, in file order (rules 1-2, 4).
  expect(messages.map((m) => m.role)).toEqual(["user", "assistant", "assistant"]);
  expect(messages[0]).toEqual({ id: "3ad4fe91", role: "user", text: "ping" });
  expect(messages[1]).toEqual({
    id: "79cefd99",
    role: "assistant",
    text: "Pong. What can I help you with?",
  });
  expect(messages[2]).toEqual({ id: "eca18d09", role: "assistant", text: "Now edit." });
});

test("parsePiSession keeps text blocks and skips thinking, toolCall and system", () => {
  const messages = parsePiSession(piJsonl());
  const text = messages.map((m) => m.text).join("\n");
  // The text blocks of a user/assistant message are kept (rule 3).
  expect(text).toContain("Pong. What can I help you with?");
  expect(text).toContain("Now edit.");
  // thinking, toolCall and system are skipped (rules 1-3).
  expect(text).not.toContain("The user is saying ping");
  expect(text).not.toContain("sed -n");
  expect(text).not.toContain("expert coding assistant");
  expect(messages.some((m) => m.role === "system")).toBe(false);
  expect(messages).toHaveLength(3);

  // A plain string `content` is the text as is (rule 3, the other content shape).
  const strung = parsePiSession(
    '{"type":"message","id":"s1","message":{"role":"user","content":"as is"}}',
  );
  expect(strung).toEqual([{ id: "s1", role: "user", text: "as is" }]);
});

test("parseTailOutput reads the cpd session tail grammar into messages", () => {
  const messages = parseTailOutput(tailOutput());
  // The header line is ignored; the prefixes map to roles; ids are synthetic in output order.
  expect(messages.map((m) => m.role)).toEqual(["user", "assistant", "system", "tool", "tool"]);
  expect(messages.map((m) => m.id)).toEqual(["tail-1", "tail-2", "tail-3", "tail-4", "tail-5"]);
  expect(messages[0]!.text).toBe("run the checks");
  expect(messages[2]!.text).toBe("session started");
  expect(messages[3]).toMatchObject({ role: "tool", name: "bash", text: "npm test -- --run" });
  expect(messages[4]).toMatchObject({
    role: "tool",
    name: "read",
    text: "ENOENT: no such file or directory",
    error: true,
  });
});

test("parseTailOutput joins multi-line texts and marks a failed tool result", () => {
  const messages = parseTailOutput(tailOutput());
  // A continuation line has no prefix and joins the previous message with a newline (rule 2).
  expect(messages.find((m) => m.role === "assistant")!.text).toBe(
    "ok, the checks pass\nand the linter is clean",
  );
  // A `tool` line ending ` (error)` is a failed result, and the marker is consumed (rule 3).
  const failed = messages[messages.length - 1]!;
  expect(failed.error).toBe(true);
  expect(failed.text).toBe("ENOENT: no such file or directory");
  expect(failed.text).not.toContain("(error)");
});

test("toSnapshotFrame maps messages to a fold snapshot", () => {
  const messages: TranscriptMessage[] = [
    { id: "tail-1", role: "user", text: "hi" },
    { id: "tail-2", role: "tool", name: "bash", text: "boom", error: true },
  ];
  // The defaults the README pins: epoch "live", seq 1, snapshot: { messages }.
  expect(toSnapshotFrame(messages)).toEqual({
    type: "snapshot",
    epoch: "live",
    seq: 1,
    snapshot: { messages },
  });
  const custom = toSnapshotFrame(messages, "e1", 7);
  expect(custom).toEqual({
    type: "snapshot",
    epoch: "e1",
    seq: 7,
    snapshot: { messages },
  });
});

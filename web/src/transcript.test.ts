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

/** One parsed part, read shape-agnostically: the contract tags parts `type`, the preview `kind`. */
type ParsedPart = Record<string, unknown>;

/** The parts of a parsed turn; none on a message the parser kept as text only. */
function partsOf(message: TranscriptMessage): ParsedPart[] {
  const parts = (message as unknown as { parts?: unknown }).parts;
  return Array.isArray(parts) ? (parts as ParsedPart[]) : [];
}

/** The part's kind, whichever discriminant the implementation tags it with. */
function kindOf(part: ParsedPart): string {
  return String(part.kind ?? part.type ?? "");
}

/** A tool part's call arguments, under either name. */
function argsOf(part: ParsedPart): Record<string, unknown> {
  const args = part.arguments ?? part.args;
  return typeof args === "object" && args !== null ? (args as Record<string, unknown>) : {};
}

/** A tool part's result text, under either name. */
function resultOf(part: ParsedPart): string {
  return String(part.result ?? part.output ?? "");
}

/** A tool part's image source: a ready data URL, or mime plus base64 to build one. */
function imageSrcOf(part: ParsedPart): string | undefined {
  if (typeof part.dataUrl === "string") return part.dataUrl;
  if (typeof part.data === "string") return `data:${String(part.mime ?? "image/png")};base64,${part.data}`;
  return undefined;
}

/** An ask part's questions: parsed onto the part, or still inside its call arguments. */
function questionsOf(part: ParsedPart): Record<string, unknown>[] {
  const found = part.questions ?? argsOf(part).questions;
  return Array.isArray(found) ? (found as Record<string, unknown>[]) : [];
}

/** An ask part's answers: parsed onto the part, or still inside the result's `details.answers`. */
function answersOf(part: ParsedPart): Record<string, unknown>[] {
  const details = part.details;
  const fromDetails =
    typeof details === "object" && details !== null
      ? (details as Record<string, unknown>).answers
      : undefined;
  const found = part.answers ?? fromDetails ?? argsOf(part).answers;
  return Array.isArray(found) ? (found as Record<string, unknown>[]) : [];
}

test("parsePiSession keeps thinking, tool calls, their results and images, and an ask's questions and answers", () => {
  const jsonl = [
    JSON.stringify({ type: "message", id: "u1", message: { role: "user", content: "ping" } }),
    JSON.stringify({
      type: "message",
      id: "a1",
      message: {
        role: "assistant",
        content: [
          { type: "thinking", thinking: "one two three four" },
          { type: "text", text: "Here." },
          { type: "toolCall", id: "call-1", name: "bash", arguments: { command: "sed -n '1,5p' docs/contract.md" } },
          { type: "image", data: "aGk=", mimeType: "image/png" },
        ],
      },
    }),
    JSON.stringify({
      type: "message",
      id: "r1",
      message: {
        role: "toolResult",
        toolCallId: "call-1",
        content: [{ type: "text", text: "line one\nline two" }],
        durationMs: 1200,
        isError: false,
      },
    }),
    JSON.stringify({
      type: "message",
      id: "a2",
      message: {
        role: "assistant",
        content: [
          {
            type: "toolCall",
            id: "call-2",
            name: "ask_user_question",
            arguments: {
              questions: [
                {
                  header: "Pick",
                  question: "Which one?",
                  options: [
                    { label: "A", description: "first" },
                    { label: "B", description: "second" },
                  ],
                },
              ],
            },
          },
        ],
      },
    }),
    JSON.stringify({
      type: "message",
      id: "r2",
      message: {
        role: "toolResult",
        toolCallId: "call-2",
        content: [{ type: "text", text: "User has answered" }],
        details: {
          answers: [
            { questionIndex: 0, question: "Which one?", kind: "option", answer: "A" },
            { questionIndex: 1, question: "Anything else?", kind: "custom", answer: "my words" },
            { questionIndex: 2, question: "Pick several?", kind: "multi", answer: null, selected: ["A", "B"] },
          ],
        },
        isError: false,
      },
    }),
    JSON.stringify({
      type: "message",
      id: "a3",
      message: {
        role: "assistant",
        content: [
          {
            type: "toolCall",
            id: "call-3",
            name: "ask_user_question",
            arguments: { questions: [{ header: "Risk", question: "Accept?", options: [{ label: "Yes" }] }] },
          },
        ],
      },
    }),
    JSON.stringify({
      type: "message",
      id: "r3",
      message: {
        role: "toolResult",
        toolCallId: "call-3",
        content: [{ type: "text", text: "User declined to answer questions" }],
        details: { answers: [], cancelled: true },
        isError: true,
      },
    }),
  ].join("\n");

  const messages = parsePiSession(jsonl);

  // The toolResult lines are no turns of their own; each pairs with its call (rule 4).
  expect(messages.map((m) => m.role)).toEqual(["user", "assistant", "assistant", "assistant"]);

  const first = messages[1]!;
  // Every block of the turn is kept, in order (rule 3).
  expect(partsOf(first).map(kindOf)).toEqual(expect.arrayContaining(["thinking", "text", "tool", "image"]));
  expect(partsOf(first).find((part) => kindOf(part) === "thinking")?.text).toBe("one two three four");

  const tool = partsOf(first).find((part) => kindOf(part) === "tool")!;
  expect(tool.name).toBe("bash");
  expect(argsOf(tool).command).toBe("sed -n '1,5p' docs/contract.md");
  // The following result lands on the call: its text and its duration.
  expect(resultOf(tool)).toContain("line two");
  expect(tool.durationMs ?? tool.duration_ms).toBe(1200);

  const image = partsOf(first).find((part) => kindOf(part) === "image")!;
  expect(imageSrcOf(image)).toBe("data:image/png;base64,aGk=");

  // The ask keeps its questions (from the call) and its answers (from the result's details).
  const asked = partsOf(messages[2]!).find((part) => kindOf(part) === "tool")!;
  expect(questionsOf(asked).map((question) => question.header)).toContain("Pick");
  const answers = answersOf(asked);
  expect(answers.map((answer) => answer.kind)).toEqual(["option", "custom", "multi"]);
  expect(answers[0]!.answer).toBe("A");
  expect(answers[1]!.answer).toBe("my words");
  expect(answers[2]!.selected).toEqual(["A", "B"]);

  // A refused ask keeps its failure and its refusal text.
  const refused = partsOf(messages[3]!).find((part) => kindOf(part) === "tool")!;
  expect(resultOf(refused)).toContain("User declined to answer questions");
  expect(refused.error === true || refused.state === "error" || refused.isError === true).toBe(true);
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

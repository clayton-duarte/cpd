// The transcript parsers and the snapshot mapping of the dev live adapter (web/README.md "The
// TypeScript contract", hs-d819). A real cpd Session renders into the existing Chat through one
// fold `snapshot` frame: a pi Session is parsed from its session file's JSONL, a durable Session
// from the plain lines of `cpd session tail` (docs/contract.md 6.20).
import type { SnapshotFrame } from "./fold/fold.ts";

/** One rendered transcript message of a real Session. */
export interface TranscriptMessage {
  id: string;
  role: "user" | "assistant" | "system" | "tool";
  /** The tool name of a `tool` message (the tail grammar's `<toolName>`). */
  name?: string;
  text: string;
  /** A failed tool result — the tail grammar's ` (error)` suffix. */
  error?: true;
}

/** One parsed pi session-file line (only the fields the parser reads). */
interface PiLine {
  type?: string;
  id?: string;
  message?: { role?: string; content?: unknown };
}

/** One content block of a pi message (only the fields the parser reads). */
interface PiBlock {
  type?: string;
  text?: string;
}

/** The text of a pi `message.content`: a plain string, or the join of its `text` blocks. */
function piContent(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .filter((block): block is PiBlock => typeof block === "object" && block !== null)
    .filter((block) => block.type === "text")
    .map((block) => block.text ?? "")
    .join("");
}

/** Parses a pi session file's JSONL text into messages (pi parse rules, web/README.md). */
export function parsePiSession(jsonl: string): TranscriptMessage[] {
  const messages: TranscriptMessage[] = [];
  for (const raw of jsonl.split("\n")) {
    if (raw.trim() === "") continue;
    // Rule 1: every line is JSON; only a `message` line is a message.
    const line = JSON.parse(raw) as PiLine;
    if (line.type !== "message") continue;
    // Rule 2: only role user/assistant; `system` is skipped.
    const role = line.message?.role;
    if (role !== "user" && role !== "assistant") continue;
    // Rule 3: a plain string, or the join of the `text` blocks; `thinking`/`toolCall` skipped.
    // Rule 4: the line's `id`, in file order, completed history (never pending).
    messages.push({ id: line.id ?? "", role, text: piContent(line.message?.content) });
  }
  return messages;
}

/** The prefix that starts a user/assistant/system line of the tail grammar (rule 1). */
const TAIL_PREFIX = /^(user|assistant|system): (.*)$/;
/** The prefix that starts a `tool <toolName>:` line of the tail grammar (rule 1). */
const TAIL_TOOL_PREFIX = /^tool ([^:]+): (.*)$/;
/** The suffix marking a failed tool result (rule 3). */
const TAIL_ERROR_SUFFIX = " (error)";

/** A tail message under construction, before its continuation lines join (rule 2). */
interface TailMessage {
  role: TranscriptMessage["role"];
  name?: string;
  lines: string[];
}

/** Parses `cpd session tail` output in the grammar of contract §6.20 (tail parse rules). */
export function parseTailOutput(output: string): TranscriptMessage[] {
  const messages: TranscriptMessage[] = [];
  let current: TailMessage | undefined;

  // Rule 3: a tool text ending ` (error)` is a failed result and the marker is consumed.
  const finalize = (): void => {
    if (current === undefined) return;
    let text = current.lines.join("\n");
    const error = current.role === "tool" && text.endsWith(TAIL_ERROR_SUFFIX);
    if (error) text = text.slice(0, -TAIL_ERROR_SUFFIX.length);
    // Rule 4: synthetic ids in output order.
    const message: TranscriptMessage = {
      id: `tail-${messages.length + 1}`,
      role: current.role,
      text,
    };
    if (current.name !== undefined) message.name = current.name;
    if (error) message.error = true;
    messages.push(message);
  };

  const lines = output.split("\n");
  // A trailing line terminator is not an extra source line.
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();

  for (const line of lines) {
    const tool = TAIL_TOOL_PREFIX.exec(line);
    const plain = TAIL_PREFIX.exec(line);
    if (plain !== null) {
      finalize();
      current = { role: plain[1] as TranscriptMessage["role"], lines: [plain[2] ?? ""] };
    } else if (tool !== null) {
      finalize();
      current = { role: "tool", name: tool[1], lines: [tool[2] ?? ""] };
    } else if (current !== undefined) {
      // Rule 2: a prefixed-less line continues the previous message with a newline.
      current.lines.push(line);
    }
    // Rule 4: lines before the first prefixed line are ignored.
  }
  finalize();
  return messages;
}

/** Maps messages onto one fold `snapshot` frame (fold rule 3): `snapshot: { messages }`. Defaults: epoch "live", seq 1. */
export function toSnapshotFrame(
  messages: readonly TranscriptMessage[],
  epoch = "live",
  seq = 1,
): SnapshotFrame {
  return { type: "snapshot", epoch, seq, snapshot: { messages } };
}

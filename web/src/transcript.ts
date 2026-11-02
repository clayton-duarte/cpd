// The transcript parsers and the snapshot mapping of the dev live adapter (web/README.md "The
// TypeScript contract", hs-d819). A real cpd Session renders into the existing Chat through one
// fold `snapshot` frame: a pi Session is parsed from its session file's JSONL, a durable Session
// from the plain lines of `cpd session tail` (docs/contract.md 6.20).
//
// A turn is not only its answer. A pi assistant message interleaves text, thinking and tool calls,
// and its tool results arrive as separate `toolResult` lines; the parser keeps every one of them as
// an ordered part so the Chat can show the whole turn (Reasoning, Tool, the answer) instead of an
// empty bubble. `text` stays the concatenation of the text parts, which is what the fold and the
// mock render.
import type { SnapshotFrame } from "./fold/fold.ts";

/** One text part of a turn: the answer, or a user's words. */
export interface TranscriptTextPart {
  kind: "text";
  text: string;
}

/** One thinking part of an assistant turn (pi's `thinking` block). */
export interface TranscriptThinkingPart {
  kind: "thinking";
  text: string;
}

/** One question of an `ask_user_question` call, as pi sends it in the call's arguments. */
export interface TranscriptAskQuestion {
  header: string;
  question: string;
  options: readonly TranscriptAskOption[];
  multiSelect: boolean;
}

/** One choice of an `ask_user_question` question. */
export interface TranscriptAskOption {
  label: string;
  description?: string;
  preview?: string;
}

/** How one question was answered: an option, a typed answer, or a multi-select. */
export interface TranscriptAskAnswer {
  questionIndex: number;
  question: string;
  kind: "option" | "custom" | "multi";
  /** The typed answer, the chosen label, or null when the question went unanswered. */
  answer: string | null;
  /** The chosen labels of a `multi` answer. */
  selected?: readonly string[];
}

/** One tool call of an assistant turn, with the result merged onto it by its call id. */
export interface TranscriptToolPart {
  kind: "tool";
  id: string;
  name: string;
  args: unknown;
  state: "running" | "done" | "error";
  output?: string;
  durationMs?: number;
  /** An `ask_user_question` call's questions, parsed from its arguments. */
  questions?: readonly TranscriptAskQuestion[];
  /** An `ask_user_question` result's answers, parsed from its `details`. */
  answers?: readonly TranscriptAskAnswer[];
}

/** One image part (pi's `image` block: a base64 payload and its mime type). */
export interface TranscriptImagePart {
  kind: "image";
  mime: string;
  data: string;
}

/** One ordered part of a turn. */
export type TranscriptPart =
  | TranscriptTextPart
  | TranscriptThinkingPart
  | TranscriptToolPart
  | TranscriptImagePart;

/** One rendered transcript message of a real Session. */
export interface TranscriptMessage {
  id: string;
  role: "user" | "assistant" | "system" | "tool";
  /** The tool name of a `tool` message (the tail grammar's `<toolName>`). */
  name?: string;
  /** The text parts joined: what the fold and the mock render. */
  text: string;
  /** Every part of the turn, in order; absent when there is nothing but `text`. */
  parts?: TranscriptPart[];
  /** A failed tool result — the tail grammar's ` (error)` suffix. */
  error?: true;
  /** Present when cpd delivered this turn into the Session rather than the person typing it. */
  delivery?: TranscriptDelivery;
}

/** One parsed pi session-file line (only the fields the parser reads). */
interface PiLine {
  type?: string;
  id?: string;
  customType?: string;
  display?: boolean;
  content?: unknown;
  details?: unknown;
  message?: { role?: string; content?: unknown };
}

/**
 * How a message reached the Session, for a record cpd delivered rather than the person typing into
 * the agent's own prompt: the wrapper cpd wrote and the reason it recorded.
 */
export interface TranscriptDelivery {
  /** The header cpd wrote, e.g. `Message from the User (13:02 EST)`. */
  header: string;
  /** The time cpd stamped on it, e.g. `13:02 EST`. */
  at: string;
  /** The recorded reason, e.g. `sent from the cpd web chat`. */
  reason?: string;
}

/**
 * The wrapper cpd writes around a delivered message: `Message from X (13:02 EST): <body>`. The
 * sender itself may carry parentheses — a Session is named with its id in them
 * (`Message from web-chat-live (14bd6551-…) (13:02 EST): …`) — so the time is the last group.
 */
const CPD_DELIVERY = /^Message from (.+?) \(([^()]*)\): ?/

/** One content block of a pi message (only the fields the parser reads). */
interface PiBlock {
  type?: string;
  text?: string;
  thinking?: string;
  id?: string;
  name?: string;
  arguments?: unknown;
  data?: string;
  mimeType?: string;
}

/** The text of a pi block list: the `text` blocks joined, in order. */
function piText(blocks: readonly PiBlock[]): string {
  return blocks
    .filter((block) => block.type === "text")
    .map((block) => block.text ?? "")
    .join("");
}

/** The content of a pi message as blocks: a plain string is one text block. */
function piBlocks(content: unknown): PiBlock[] {
  if (typeof content === "string") return [{ type: "text", text: content }];
  if (!Array.isArray(content)) return [];
  return content.filter(
    (block): block is PiBlock => typeof block === "object" && block !== null,
  );
}

/**
 * The user turn a cpd `session-message` record holds, or nothing when the line is another kind of
 * cpd record (an event, a summary, an escalation) — those are engine notices.
 */
function cpdDelivery(line: PiLine, content: string): TranscriptMessage | undefined {
  const details = isRecord(line.details) ? line.details : undefined;
  if (details?.kind !== "session-message") return undefined;
  const data = isRecord(details.data) ? details.data : undefined;
  const header = CPD_DELIVERY.exec(content);
  // cpd records the person's own words in `data.text`, unwrapped; fall back to the wrapper's body.
  const body =
    typeof data?.text === "string" && data.text !== ""
      ? data.text
      : header === null
        ? content
        : content.slice(header[0].length);
  const message: TranscriptMessage = {
    id: line.id ?? "",
    role: "user",
    text: body,
    parts: [{ kind: "text", text: body }],
  };
  if (header !== null) {
    message.delivery = {
      header: `Message from ${header[1]} (${header[2]})`,
      at: header[2] ?? "",
      ...(typeof data?.unblocks === "string" && data.unblocks !== ""
        ? { reason: data.unblocks }
        : {}),
    };
  }
  return message;
}

/** True for a JSON object, the only shape an untrusted arguments or details value may have. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** The questions of an `ask_user_question` call's arguments; anything unknown parses to none. */
export function parseAskQuestions(args: unknown): TranscriptAskQuestion[] {
  if (!isRecord(args) || !Array.isArray(args.questions)) return [];
  const questions: TranscriptAskQuestion[] = [];
  for (const raw of args.questions) {
    if (!isRecord(raw) || typeof raw.question !== "string") continue;
    const options: TranscriptAskOption[] = [];
    if (Array.isArray(raw.options)) {
      for (const option of raw.options) {
        if (!isRecord(option) || typeof option.label !== "string") continue;
        options.push({
          label: option.label,
          ...(typeof option.description === "string" ? { description: option.description } : {}),
          ...(typeof option.preview === "string" ? { preview: option.preview } : {}),
        });
      }
    }
    questions.push({
      header: typeof raw.header === "string" ? raw.header : "",
      question: raw.question,
      options,
      multiSelect: raw.multiSelect === true,
    });
  }
  return questions;
}

/** The answers of an `ask_user_question` result's `details`; anything unknown parses to none. */
export function parseAskAnswers(details: unknown): TranscriptAskAnswer[] {
  if (!isRecord(details) || !Array.isArray(details.answers)) return [];
  const answers: TranscriptAskAnswer[] = [];
  for (const raw of details.answers) {
    if (!isRecord(raw) || typeof raw.questionIndex !== "number") continue;
    const kind =
      raw.kind === "multi" ? "multi" : raw.kind === "custom" ? "custom" : "option";
    answers.push({
      questionIndex: raw.questionIndex,
      question: typeof raw.question === "string" ? raw.question : "",
      kind,
      answer: typeof raw.answer === "string" ? raw.answer : null,
      ...(Array.isArray(raw.selected)
        ? { selected: raw.selected.filter((label): label is string => typeof label === "string") }
        : {}),
    });
  }
  return answers;
}

/** The parts of one pi content block list, in order. */
function piParts(blocks: readonly PiBlock[]): TranscriptPart[] {
  const parts: TranscriptPart[] = [];
  for (const block of blocks) {
    switch (block.type) {
      case "text":
        parts.push({ kind: "text", text: block.text ?? "" });
        break;
      case "thinking":
        parts.push({ kind: "thinking", text: block.thinking ?? "" });
        break;
      case "toolCall": {
        // An ask carries its questions in the arguments, so the view renders a question card
        // rather than a JSON blob; an unknown shape parses to none and stays a plain tool row.
        const questions =
          block.name === "ask_user_question" ? parseAskQuestions(block.arguments) : [];
        parts.push({
          kind: "tool",
          id: block.id ?? "",
          name: block.name ?? "tool",
          args: block.arguments,
          state: "running",
          ...(questions.length > 0 ? { questions } : {}),
        });
        break;
      }
      case "image":
        if (block.data !== undefined) {
          parts.push({
            kind: "image",
            mime: block.mimeType ?? "image/png",
            data: block.data,
          });
        }
        break;
      default:
        // A block this parser does not know is not rendered; it is never invented as text.
        break;
    }
  }
  return parts;
}

/** Every tool part of a message list, by call id, so a result can be merged onto its call. */
function toolParts(messages: readonly TranscriptMessage[]): Map<string, TranscriptToolPart> {
  const byId = new Map<string, TranscriptToolPart>();
  for (const message of messages) {
    for (const part of message.parts ?? []) {
      if (part.kind === "tool") byId.set(part.id, part);
    }
  }
  return byId;
}

/** Parses a pi session file's JSONL text into messages (pi parse rules, web/README.md). */
export function parsePiSession(jsonl: string): TranscriptMessage[] {
  const messages: TranscriptMessage[] = [];
  const tools = new Map<string, TranscriptToolPart>();

  for (const raw of jsonl.split("\n")) {
    if (raw.trim() === "") continue;
    // Rule 1: every line is JSON; a `message` line is a turn and a displayed `custom_message` is
    // a notice.
    const line = JSON.parse(raw) as PiLine;

    if (line.type === "custom_message") {
      if (line.display === false) continue;
      const text = typeof line.content === "string" ? line.content : "";
      if (text === "") continue;
      // Rule 1a: cpd records both its own events and the messages delivered into a Session as
      // `custom_message` lines. A `session-message` is a person talking to the agent — it is a user
      // turn, and its words sit under cpd's wrapper; every other kind is an engine notice.
      const delivered = cpdDelivery(line, text);
      if (delivered !== undefined) {
        messages.push(delivered);
        continue;
      }
      messages.push({
        id: line.id ?? `notice-${messages.length + 1}`,
        role: "system",
        text,
        parts: [{ kind: "text", text }],
      });
      continue;
    }

    if (line.type !== "message") continue;
    const role = line.message?.role;

    // Rule 2: `system` is the prompt, not a turn; a `toolResult` is merged onto the tool call it
    // answers, so it never becomes a turn of its own.
    if (role === "toolResult") {
      const result = line.message as {
        content?: unknown;
        toolCallId?: string;
        isError?: boolean;
        durationMs?: number;
        details?: unknown;
      };
      const part = tools.get(result.toolCallId ?? "");
      if (part !== undefined) {
        part.state = result.isError === true ? "error" : "done";
        part.output = piText(piBlocks(result.content));
        if (typeof result.durationMs === "number") {
          part.durationMs = result.durationMs;
        }
        if (part.questions !== undefined) {
          const answers = parseAskAnswers(result.details);
          if (answers.length > 0) part.answers = answers;
        }
      }
      continue;
    }
    if (role !== "user" && role !== "assistant") continue;

    // Rule 3: the turn's parts in order — text, thinking, tool calls, images.
    const blocks = piBlocks(line.message?.content);
    const parts = piParts(blocks);
    const message: TranscriptMessage = {
      // Rule 4: the line's `id`, in file order, completed history (never pending).
      id: line.id ?? "",
      role,
      text: piText(blocks),
    };
    if (parts.length > 0) message.parts = parts;
    messages.push(message);
    // A tool call is registered as soon as it is seen, so a later result finds it.
    for (const part of parts) {
      if (part.kind === "tool") tools.set(part.id, part);
    }
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
    // A tail `tool` line is a finished call: its text is the result, never a call's arguments.
    if (current.role === "tool") {
      message.parts = [
        {
          kind: "tool",
          id: `tail-tool-${messages.length + 1}`,
          name: current.name ?? "tool",
          args: undefined,
          state: error ? "error" : "done",
          output: text,
        },
      ];
    } else {
      message.parts = [{ kind: "text", text }];
    }
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

/** The tool parts of a message, whatever its shape: what the Chat renders as Tool cards. */
export function toolPartsOf(message: TranscriptMessage): TranscriptToolPart[] {
  return (message.parts ?? []).filter(
    (part): part is TranscriptToolPart => part.kind === "tool",
  );
}

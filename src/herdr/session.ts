import { closeSync, openSync, readFileSync, readSync } from "node:fs";
import { z } from "zod";
import { failure, notFound } from "../lib/errors.ts";

const Header = z.looseObject({
  type: z.literal("session"),
  id: z.string().min(1),
  cwd: z.string(),
  parentSession: z.string().nullish(),
});
/** Every line of a pi session is an object with a `type`; types we do not read are ignored. */
const Entry = z.looseObject({ type: z.string() });
const MessageEntry = z.looseObject({ message: z.looseObject({ role: z.string() }) });
const Assistant = z.looseObject({
  stopReason: z.string(),
  content: z.union([z.string(), z.array(z.looseObject({ type: z.string(), text: z.string().optional() }))]),
});

export type SessionHeader = { id: string; cwd: string; parentSession?: string };

function malformed(path: string, line: number, reason: string) {
  return failure("session_malformed", `${path}:${line}: ${reason}`, { path, line });
}

function missing(path: string, e: unknown) {
  if ((e as NodeJS.ErrnoException).code === "ENOENT") {
    return notFound("session_not_found", `session file does not exist (yet): ${path}`, { path });
  }
  return e;
}

function firstLine(path: string): string {
  let fd: number;
  try {
    fd = openSync(path, "r");
  } catch (e) {
    throw missing(path, e);
  }
  try {
    const buf = Buffer.alloc(65536);
    let text = "";
    for (;;) {
      const n = readSync(fd, buf, 0, buf.length, null);
      text += buf.toString("utf8", 0, n);
      const nl = text.indexOf("\n");
      if (nl !== -1) return text.slice(0, nl);
      if (n === 0) return text;
    }
  } finally {
    closeSync(fd);
  }
}

/** The session header (first line): `{id, cwd, parentSession?}`. */
export function readSessionHeader(path: string): SessionHeader {
  const line = firstLine(path);
  let json: unknown;
  try {
    json = JSON.parse(line);
  } catch (e) {
    throw malformed(path, 1, `invalid JSON: ${(e as Error).message}`);
  }
  const h = Header.safeParse(json);
  if (!h.success) throw malformed(path, 1, `not a session header: ${z.prettifyError(h.error)}`);
  const { id, cwd, parentSession } = h.data;
  return { id, cwd, ...(parentSession ? { parentSession } : {}) };
}

/**
 * The last assistant message: `stopReason` ("stop" when the turn finished, "toolUse" when the process was
 * killed mid-turn, t1 F7) and its concatenated text blocks. Null when there is no assistant message yet.
 * Unknown entry types are ignored; a malformed line fails loud (`session_malformed`).
 */
export function lastAssistant(path: string): { stopReason: string; text: string } | null {
  let content: string;
  try {
    content = readFileSync(path, "utf8");
  } catch (e) {
    throw missing(path, e);
  }
  const lines = content.split("\n");
  if (lines.at(-1) === "") lines.pop();
  let last: { stopReason: string; text: string } | null = null;
  lines.forEach((text, i) => {
    const n = i + 1;
    let json: unknown;
    try {
      json = JSON.parse(text);
    } catch (e) {
      throw malformed(path, n, `invalid JSON: ${(e as Error).message}`);
    }
    const entry = Entry.safeParse(json);
    if (!entry.success) throw malformed(path, n, `not a session entry: ${z.prettifyError(entry.error)}`);
    if (entry.data.type !== "message") return;
    const msg = MessageEntry.safeParse(json);
    if (!msg.success) throw malformed(path, n, `message entry without a role: ${z.prettifyError(msg.error)}`);
    if (msg.data.message.role !== "assistant") return;
    const a = Assistant.safeParse(msg.data.message);
    if (!a.success) throw malformed(path, n, `bad assistant message: ${z.prettifyError(a.error)}`);
    last = {
      stopReason: a.data.stopReason,
      text:
        typeof a.data.content === "string"
          ? a.data.content
          : a.data.content.flatMap((b) => (b.type === "text" && b.text !== undefined ? [b.text] : [])).join("\n"),
    };
  });
  return last;
}

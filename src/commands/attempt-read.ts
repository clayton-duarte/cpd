import { readFileSync } from "node:fs";
import { z } from "zod";
import { findAttempt } from "../ledger/attempts.ts";
import { readLedger } from "../ledger/bd.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { failure, notFound, usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { assertAttemptId } from "../lib/paths.ts";

// Contract §11 `cpd attempt read`: a debug view of the Worker's pi session (assistant text and tool-call names).

const DEFAULT_LINES = 40;

const Entry = z.looseObject({ type: z.string() });
const Message = z.looseObject({
  message: z.looseObject({
    role: z.string(),
    content: z.union([z.string(), z.array(z.looseObject({ type: z.string(), text: z.string().optional(), name: z.string().optional() }))]).optional(),
  }),
});

/** One line per assistant text line (`assistant: …`) and per tool call (`tool: <name>`), in session order. */
export function transcriptLines(path: string): string[] {
  let content: string;
  try {
    content = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") throw notFound("session_not_found", `session file does not exist (yet): ${path}`, { path });
    throw e;
  }
  const lines = content.split("\n");
  // A session being written may end in a partial line (no trailing newline): ignore only that one.
  const partial = content.endsWith("\n") ? null : lines.length - 1;
  if (lines.at(-1) === "") lines.pop();
  const out: string[] = [];
  lines.forEach((text, i) => {
    let json: unknown;
    try {
      json = JSON.parse(text);
    } catch (e) {
      if (i === partial) return;
      throw failure("session_malformed", `${path}:${i + 1}: invalid JSON: ${(e as Error).message}`, { path, line: i + 1 });
    }
    const entry = Entry.safeParse(json);
    if (!entry.success) throw failure("session_malformed", `${path}:${i + 1}: not a session entry`, { path, line: i + 1 });
    if (entry.data.type !== "message") return;
    const m = Message.safeParse(json);
    if (!m.success) throw failure("session_malformed", `${path}:${i + 1}: message entry without a role`, { path, line: i + 1 });
    if (m.data.message.role !== "assistant") return;
    const content = m.data.message.content ?? [];
    if (typeof content === "string") {
      for (const l of content.split("\n")) out.push(`assistant: ${l}`);
      return;
    }
    for (const block of content) {
      if (block.type === "text" && block.text !== undefined) for (const l of block.text.split("\n")) out.push(`assistant: ${l}`);
      else if (block.type === "toolCall" && block.name !== undefined) out.push(`tool: ${block.name}`);
    }
  });
  return out;
}

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand("attempt read", argv, { wf: { type: "string" }, lines: { type: "string" } }, 1);
  const id = positionals[0];
  if (id === undefined) throw usage("usage", "cpd attempt read: pass the Attempt id");
  assertAttemptId(id);
  let n = DEFAULT_LINES;
  if (values.lines !== undefined) {
    if (!/^[1-9][0-9]*$/.test(values.lines)) throw usage("usage", `cpd attempt read: --lines must be a positive integer, got ${JSON.stringify(values.lines)}`);
    n = Number(values.lines);
  }
  const wf = resolveWorkflowId(values.wf);
  const { attempt } = await findAttempt(readLedger(wf), id);
  if (attempt.sessionPath === null) throw notFound("session_not_found", `Attempt ${id} has no Worker session recorded yet`, { attempt: id });
  const all = transcriptLines(attempt.sessionPath);
  return { ok: true, attempt: id, session: attempt.sessionPath, total: all.length, lines: all.slice(-n) };
}

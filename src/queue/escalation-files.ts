import { linkSync, mkdirSync, readdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { failure } from "../lib/errors.ts";
import { pidAlive } from "../lib/lock.ts";
import { assertAttemptId, escalationAnswerPath, escalationId, escalationQuestionPath, escalationsDir } from "../lib/paths.ts";

export const QuestionSchema = z.strictObject({
  v: z.literal(1),
  id: z.string().min(1),
  wf: z.string().min(1),
  attempt: z.string().min(1),
  question: z.string().min(1),
  askedAt: z.string().min(1),
  askerPid: z.number().int().positive(),
});
export type Question = z.infer<typeof QuestionSchema>;

export const AnswerSchema = z.strictObject({
  v: z.literal(1),
  id: z.string().min(1),
  answer: z.string(),
  answeredAt: z.string().min(1),
});
export type Answer = z.infer<typeof AnswerSchema>;

/** Create `path` with `content`, atomically and exclusively. Returns false if it already exists. */
function writeExclusive(path: string, content: string): boolean {
  const tmp = `${path}.${process.pid}.tmp`;
  writeFileSync(tmp, content);
  try {
    linkSync(tmp, path);
    return true;
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "EEXIST") return false;
    throw e;
  } finally {
    unlinkSync(tmp);
  }
}

function readJson<T>(path: string, schema: z.ZodType<T>): T | null {
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw failure("escalation_unreadable", `cannot read ${path}: ${(e as Error).message}`, { path });
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    throw failure("escalation_invalid", `not valid JSON: ${path}`, { path, parseError: (e as Error).message });
  }
  const parsed = schema.safeParse(raw);
  if (!parsed.success) throw failure("escalation_invalid", `does not match the schema: ${path}`, { path, issues: parsed.error.issues });
  return parsed.data;
}

/** Write a question; false if one with this id already exists. */
export function writeQuestion(wf: string, q: Question): boolean {
  const parsed = QuestionSchema.parse(q);
  mkdirSync(escalationsDir(wf), { recursive: true });
  return writeExclusive(escalationQuestionPath(wf, parsed.id), JSON.stringify(parsed, null, 2) + "\n");
}

export function readQuestion(wf: string, escId: string): Question | null {
  return readJson(escalationQuestionPath(wf, escId), QuestionSchema);
}

/** Write an answer; false if the id was already answered. */
export function writeAnswer(wf: string, a: Answer): boolean {
  const parsed = AnswerSchema.parse(a);
  mkdirSync(escalationsDir(wf), { recursive: true });
  return writeExclusive(escalationAnswerPath(wf, parsed.id), JSON.stringify(parsed, null, 2) + "\n");
}

export function readAnswer(wf: string, escId: string): Answer | null {
  return readJson(escalationAnswerPath(wf, escId), AnswerSchema);
}

export function removeQuestion(wf: string, escId: string): void {
  try {
    unlinkSync(escalationQuestionPath(wf, escId));
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code !== "ENOENT") throw e;
  }
}

function files(wf: string): string[] {
  try {
    return readdirSync(escalationsDir(wf));
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return [];
    throw e;
  }
}

/** Questions that have no answer yet, ordered by id. */
export function listOpenQuestions(wf: string): Question[] {
  const names = files(wf);
  const answered = new Set(names.filter((n) => n.endsWith(".a.json")).map((n) => n.slice(0, -".a.json".length)));
  const out: Question[] = [];
  for (const n of names.filter((n) => n.endsWith(".q.json")).sort()) {
    const id = n.slice(0, -".q.json".length);
    if (answered.has(id)) continue;
    const q = readQuestion(wf, id);
    if (q) out.push(q);
  }
  return out;
}

/** `<attempt-id>-q<k>`, k one past the highest existing for this Attempt. */
export function nextEscalationId(wf: string, attemptId: string): string {
  assertAttemptId(attemptId);
  const prefix = `${attemptId}-q`;
  let max = 0;
  for (const n of files(wf)) {
    const m = n.startsWith(prefix) ? /^(\d+)\.[qa]\.json$/.exec(n.slice(prefix.length)) : null;
    if (m) max = Math.max(max, Number(m[1]));
  }
  return escalationId(attemptId, max + 1);
}

/** Remove open questions whose asker is dead. Returns the removed ids. */
export function expireOrphans(wf: string): string[] {
  const removed: string[] = [];
  for (const q of listOpenQuestions(wf)) {
    if (pidAlive(q.askerPid)) continue;
    removeQuestion(wf, q.id);
    removed.push(q.id);
  }
  return removed;
}

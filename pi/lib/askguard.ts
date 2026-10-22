/** commands.md §15.13: which `ask_user_question` calls are refused (G2 a long question; G3 and G4 for a Lead or Manager: no visible text this turn, labels the chat never showed). Pure: the adapter (`adapter.ts`) applies it to a `tool_call`. */

/** The most characters one `ask_user_question` call may carry, summed over its text. */
export const LIMIT = 1500;

/** G2's reason, one line (§15.13). */
export const longReason = (characters: number): string =>
  `cpd: ask_user_question is for a few short choices; put anything larger in plannotator; refused: ${characters} characters (limit ${LIMIT})`;

const length = (v: unknown): number => (typeof v === "string" ? v.length : 0);

/** The characters of a call's `input`: every question's `question` and `header`, every option's `label`, `description` and `preview`. A part that is not the documented shape counts nothing. */
export function textLength(input: unknown): number {
  const questions = (input as { questions?: unknown } | null | undefined)?.questions;
  if (!Array.isArray(questions)) return 0;
  let total = 0;
  for (const q of questions) {
    if (typeof q !== "object" || q === null) continue;
    total += length(q.question) + length(q.header);
    if (!Array.isArray(q.options)) continue;
    for (const o of q.options) {
      if (typeof o === "object" && o !== null) total += length(o.label) + length(o.description) + length(o.preview);
    }
  }
  return total;
}

/** G2: the reason when the call's text is over {@link LIMIT}, else undefined. */
export function tooLong(input: unknown): string | undefined {
  const n = textLength(input);
  return n > LIMIT ? longReason(n) : undefined;
}

/** G3's reason, one line (§15.13). */
export const noTextReason = "cpd: write what you ask about in the chat as text, then ask again; refused: no visible text in this turn";

/** G4's reason, one line (§15.13). */
export const unseenReason = (labels: string[]): string =>
  `cpd: write what you ask about in the chat as text, then ask again; refused: ${labels.join(", ")} not in the visible chat`;

type Rec = Record<string, unknown>;
const isRec = (v: unknown): v is Rec => typeof v === "object" && v !== null;

/** The strings of a call's question text (the fields {@link textLength} counts), in order. */
function texts(input: unknown): string[] {
  const out: string[] = [];
  const questions = (input as { questions?: unknown } | null | undefined)?.questions;
  if (!Array.isArray(questions)) return out;
  for (const q of questions) {
    if (!isRec(q)) continue;
    out.push(...[q.question, q.header].filter((s): s is string => typeof s === "string"));
    if (!Array.isArray(q.options)) continue;
    for (const o of q.options) if (isRec(o)) out.push(...[o.label, o.description, o.preview].filter((s): s is string => typeof s === "string"));
  }
  return out;
}

/** The labels a text cites, in order of first citation; a range X<m>–X<n> cites each of X<m>..X<n>. */
export function labelsIn(text: string): string[] {
  const out: string[] = [];
  const add = (l: string) => out.includes(l) || out.push(l);
  const re = /\b([A-Z])([0-9]+)(?:\s?[–-]\s?\1([0-9]+))?\b/g;
  for (const m of text.matchAll(re)) {
    const from = Number(m[2]);
    const to = m[3] === undefined ? NaN : Number(m[3]);
    if (from < to) for (let i = from; i <= to; i++) add(`${m[1]}${i}`);
    else {
      add(`${m[1]}${m[2]}`);
      if (m[3] !== undefined) add(`${m[1]}${m[3]}`);
    }
  }
  return out;
}

/** Text of a message content: a string, or its `text` blocks (before `stopAt`, when given). */
function textOf(content: unknown, stopAt?: (b: Rec) => boolean): string[] {
  if (typeof content === "string") return [content];
  if (!Array.isArray(content)) return [];
  const out: string[] = [];
  for (const b of content) {
    if (!isRec(b)) continue;
    if (stopAt?.(b)) break;
    if (b.type === "text" && typeof b.text === "string") out.push(b.text);
  }
  return out;
}

/** G3 and G4: the reason when the call cites no visible text this turn, or labels the chat never showed; else undefined. `branch` is the session's branch, `toolCallId` the call's id. */
export function unseen(input: unknown, toolCallId: string, branch: readonly unknown[]): string | undefined {
  const isCall = (b: Rec) => b.type === "toolCall" && b.id === toolCallId;
  let at = branch.findLastIndex((e) => isRec(e) && isRec(e.message) && Array.isArray(e.message.content) && e.message.content.some((b) => isRec(b) && isCall(b)));
  if (at < 0) at = branch.length;
  let turn = 0;
  const visible: string[] = [];
  let turnText = false;
  for (let i = 0; i < at + 1 && i < branch.length; i++) {
    const e = branch[i];
    if (!isRec(e)) continue;
    const m = isRec(e.message) ? e.message : undefined;
    const stop = i === at ? isCall : undefined;
    let parts: string[] = [];
    if (e.type === "custom_message") {
      if (i < at) turn = i + 1;
      if (e.display === true) parts = textOf(e.content);
    } else if (m?.role === "user") {
      if (i < at) turn = i + 1;
      parts = textOf(m.content);
    } else if (m?.role === "toolResult") parts = textOf(m.content);
    else if (m?.role === "assistant") {
      parts = textOf(m.content, stop);
      if (i >= turn && parts.some((p) => p.trim() !== "")) turnText = true;
    }
    visible.push(...parts);
    if (i < turn) turnText = false;
  }
  if (!turnText) return noTextReason;
  const shown = new Set<string>();
  for (const v of visible) for (const m of v.matchAll(/\b[A-Z][0-9]+\b/g)) shown.add(m[0]);
  const missing = texts(input).flatMap(labelsIn).filter((l, i, a) => a.indexOf(l) === i && !shown.has(l));
  return missing.length ? unseenReason(missing) : undefined;
}

/** commands.md §15.13: which `ask_user_question` calls are refused (G2 a long question; G4 for a Lead or Manager: labels stated nowhere). Pure: the adapter (`adapter.ts`) applies it to a `tool_call`. */

/** The most characters one `ask_user_question` call may carry, summed over its text. */
export const LIMIT = 1500;

/** G2's reason, one line (§15.13). */
export const longReason = (characters: number): string =>
  `φ: ask_user_question is for a few short choices; put anything larger in plannotator; refused: ${characters} characters (limit ${LIMIT})`;

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

/** G4's reason, one line (§15.13). */
export const unseenReason = (labels: string[]): string =>
  `φ: state what you ask about in the question itself (its text, or an option's description or preview); refused: ${labels.join(", ")} stated nowhere`;

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

/** G4: the reason when a cited label is stated nowhere (not in the call's text, not in the visible chat); else undefined. `branch` is the session's branch, `toolCallId` the call's id; a nested (codemode) call passes its parent call's id. */
export function unseen(input: unknown, toolCallId: string, branch: readonly unknown[]): string | undefined {
  const isCall = (b: Rec) => b.type === "toolCall" && b.id === toolCallId;
  let at = branch.findLastIndex((e) => isRec(e) && isRec(e.message) && Array.isArray(e.message.content) && e.message.content.some((b) => isRec(b) && isCall(b)));
  if (at < 0) at = branch.length;
  const visible: string[] = [...texts(input)];
  for (let i = 0; i < at + 1 && i < branch.length; i++) {
    const e = branch[i];
    if (!isRec(e)) continue;
    const m = isRec(e.message) ? e.message : undefined;
    if (e.type === "custom_message") {
      if (e.display === true) visible.push(...textOf(e.content));
    } else if (m?.role === "user" || m?.role === "toolResult") visible.push(...textOf(m.content));
    else if (m?.role === "assistant") visible.push(...textOf(m.content, i === at ? isCall : undefined));
  }
  const shown = new Set<string>();
  for (const v of visible) for (const m of v.matchAll(/\b[A-Z][0-9]+\b/g)) shown.add(m[0]);
  const missing = texts(input).flatMap(labelsIn).filter((l, i, a) => a.indexOf(l) === i && !shown.has(l));
  return missing.length ? unseenReason(missing) : undefined;
}

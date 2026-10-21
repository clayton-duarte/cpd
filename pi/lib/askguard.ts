/** commands.md §15.13: which `ask_user_question` calls are refused (G2 only: a long question). Pure: the adapter (`adapter.ts`) applies it to a `tool_call`. */

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

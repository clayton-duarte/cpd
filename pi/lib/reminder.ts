/** commands.md §15.8: the turn-end reminder. Pure: what to check, and the message. */

import { CpdError, isObject } from "./cpd.ts";

export const REMINDER_TYPE = "cpd-reminder";

export const REMINDER_TEXT =
  'cpd reminder: your last message spoke of further work but named no registered Job or Workflow. Register it now (cpd job add <k> --draft --title "…", or cpd workflow add …) and name its key.';

/** The fixed phrase list of §15.8, matched case-insensitively as substrings. Extending it is a change to that section. */
export const PHRASES: readonly string[] = [
  "next batch",
  "next step",
  "next steps",
  "next pr",
  "next workflow",
  "follow-up",
  "follow up",
  "followup",
  "later",
  "then i will",
  "then i'll",
  "we still need",
  "still need to",
  "still to do",
  "todo",
  "to do next",
  "once that merges",
  "once it merges",
  "after that merges",
  "after it merges",
];

/** `after`, 1 to 60 characters that are not a newline or a full stop, then `merge` or `merges`, all as whole words. */
const AFTER_MERGES = /\bafter\b[^\n.]{1,60}?\bmerges?\b/i;

export const talksOfFutureWork = (text: string): boolean => {
  const lower = text.toLowerCase();
  return PHRASES.some((p) => lower.includes(p)) || AFTER_MERGES.test(text);
};

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Whether `text` contains `key` as a whole word, case-sensitive (a key's own `-` and `_` are word characters). */
export const namesKey = (text: string, key: string): boolean => new RegExp(`(?<![\\w-])${escape(key)}(?![\\w-])`).test(text);

/** Every Job key and Workflow key `cpd plan show` lists, whatever their status. A shape that is not the documented one is an error: the check then adds nothing. */
export function planKeys(show: Record<string, unknown>): string[] {
  const keys: string[] = [];
  for (const field of ["jobs", "workflows"]) {
    const list = show[field];
    if (!Array.isArray(list)) throw new CpdError("bad_output", `cpd plan show: \`${field}\` is not a list`);
    for (const item of list) {
      if (!isObject(item) || typeof item.key !== "string") throw new CpdError("bad_output", `cpd plan show: an entry of \`${field}\` has no key`);
      keys.push(item.key);
    }
  }
  return keys;
}

/** The reminder is due: the text talks of future work and names none of the Plan's Job or Workflow keys. */
export const needsReminder = (text: string, keys: Iterable<string>): boolean => {
  if (!talksOfFutureWork(text)) return false;
  for (const k of keys) if (k !== "" && namesKey(text, k)) return false;
  return true;
};

/** The text of the last assistant message of a run (pi's `agent_end` `messages`), or `undefined` when it has none. */
export function lastAssistantText(messages: unknown): string | undefined {
  if (!Array.isArray(messages)) return undefined;
  for (let i = messages.length - 1; i >= 0; i--) {
    const m: unknown = messages[i];
    if (typeof m !== "object" || m === null || (m as { role?: unknown }).role !== "assistant") continue;
    const content = (m as { content?: unknown }).content;
    const text =
      typeof content === "string"
        ? content
        : Array.isArray(content)
          ? content.map((c) => (typeof c === "object" && c !== null && (c as { type?: unknown }).type === "text" && typeof (c as { text?: unknown }).text === "string" ? (c as { text: string }).text : "")).filter((t) => t !== "").join("\n")
          : "";
    return text.trim() === "" ? undefined : text;
  }
  return undefined;
}

/** Stub: the failed Jobs of `cpd plan show`, in list order, with their attempts count. */
export function unaddressedFailedJobs(_show: Record<string, unknown>): { key: string; attempts: number }[] {
  return [];
}

/** Stub: the failed-Job reminder text. */
export function failedJobReminderText(_keys: string[]): string {
  return "";
}

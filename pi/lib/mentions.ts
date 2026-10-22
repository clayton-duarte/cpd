import { isObject } from "./cpd.ts";

/** commands.md §15.14: magenta mentions of Leads and Managers. */
export type PaneRole = "lead" | "manager" | "worker";

const WORD = /[A-Za-z0-9_-]+/g;
const MAGENTA = (s: string) => `\x1b[35m${s}\x1b[39m`;

/** A mention is a whole token (word characters `[A-Za-z0-9_-]`) that is an id of the set. */
export function isMention(token: string, ids: ReadonlySet<string>): boolean {
  return ids.has(token);
}

/** Plain text: each whole token that is an id becomes magenta; a Worker's pane draws none. */
export function paintMentions(text: string, ids: ReadonlySet<string>, role: PaneRole = "lead"): string {
  if (role === "worker" || ids.size === 0) return text;
  return text.replace(WORD, (token) => (isMention(token, ids) ? MAGENTA(token) : token));
}

const SGR = /(\x1b\[[0-9;]*m)/;
const isFgSgr = (code: string) => /^\x1b\[(?:3[0-79]|9[0-7]|38;[0-9;]*|0?)m$/.test(code);

/**
 * One rendered (ANSI) line: only the text runs between escapes are scanned, and the runs drawn in a code colour
 * (`codeOpens`, the theme's `mdCode` and `mdCodeBlock` foregrounds) are left alone.
 */
export function paintLine(line: string, ids: ReadonlySet<string>, codeOpens: readonly string[], role: PaneRole = "lead"): string {
  if (role === "worker" || ids.size === 0) return line;
  let inCode = false;
  return line
    .split(SGR)
    .map((part, i) => {
      if (i % 2 === 1) {
        if (codeOpens.includes(part)) inCode = true;
        else if (isFgSgr(part)) inCode = false;
        return part;
      }
      return inCode ? part : paintMentions(part, ids, role);
    })
    .join("");
}

/**
 * The ids of Leads and Managers: each Session's UUID and its 8-character prefix, and the Plan and Context ids
 * (from `plan list` and `context list`) that a Lead or Manager Session names.
 */
export function mentionIds(sessions: unknown, plans: unknown, contexts: unknown): Set<string> {
  const ids = new Set<string>();
  const plansOf = new Set<string>();
  const contextsOf = new Set<string>();
  for (const s of Array.isArray(sessions) ? sessions : []) {
    if (!isObject(s) || typeof s.id !== "string") continue;
    if (s.role !== "lead" && s.role !== "manager") continue;
    ids.add(s.id);
    ids.add(s.id.slice(0, 8));
    if (typeof s.plan === "string") plansOf.add(s.plan);
    if (typeof s.context === "string") contextsOf.add(s.context);
  }
  for (const p of Array.isArray(plans) ? plans : []) if (isObject(p) && typeof p.id === "string" && plansOf.has(p.id)) ids.add(p.id);
  for (const c of Array.isArray(contexts) ? contexts : []) if (isObject(c) && typeof c.id === "string" && contextsOf.has(c.id)) ids.add(c.id);
  return ids;
}

/** The hook the assistant render override of compact-tools reads; shared through `globalThis` as both are separate extensions. */
export const PAINTER_KEY = Symbol.for("cpd.mentions.painter");
export type Painter = (line: string) => string;

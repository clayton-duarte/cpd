/**
 * The refusal miner (contract §5.7; pi/tools/refusal-miner/README.md is the worked contract).
 *
 * Pure: the contents of pi session files plus a `since` cutoff in, the ranking tables out. It reads
 * no store, runs no command and writes no file.
 *
 * Two families of refusal:
 *  - an adapter guard refusal: a `toolResult` whose first content `text` starts with `φ: `
 *    (U+03C6 + a space), `isError: true`, `details: {}` — the call never ran; the guard is the
 *    text's exact prefix (README's guard table).
 *  - a CLI refusal: a `bash`/`codemode` `toolResult` whose output carries the envelope
 *    `{"ok":false,"error":{"code":"<code>",…}}` at the start of a line, counted once per envelope.
 * A refusal only quoted inside another output (an envelope not at the start of a line, or the whole
 * output of a pure evidence reader) is not counted.
 *
 * A record counts when its top-level `timestamp` (ISO-8601, UTC `Z`) is at or after `since`.
 *
 * Role is read from the session itself: the loaded cpd skill (`cpd-lead` → Lead, `cpd-manager` →
 * Manager); otherwise a session whose `cwd` is an explore directory (`…/run/<plan>/explore`) is a
 * scout, and every other session is a Worker. The model is the session's last `model_change`
 * (`provider/modelId`) at or before the refusal.
 */

export type Role = "Manager" | "Lead" | "Worker" | "scout";

/** One session file: the file's name (its directory key and file) and its raw contents. */
export interface SessionFile {
  name: string;
  text: string;
}

export interface GuardRow {
  /** `φ: <guard>` for an adapter guard, `cli: <code>` for a CLI refusal. */
  guard: string;
  /** The exact matched string the row quotes: the refusal prefix, or the envelope shape. */
  sample: string;
  count: number;
}

export interface RoleRow {
  guard: string;
  role: Role;
  count: number;
}

export interface ModelRow {
  guard: string;
  model: string;
  count: number;
}

export interface Tables {
  /** Highest count first, then guard name. */
  byGuard: GuardRow[];
  /** Guard name, then role in the order Manager, Lead, Worker, scout. */
  byRole: RoleRow[];
  /** Guard name, then model. */
  byModel: ModelRow[];
  total: number;
}

/** Mine the refusals in `files` whose top-level timestamp is at or after `options.since`. */
export function mineRefusals(_files: SessionFile[], _options: { since: string }): Tables {
  return { byGuard: [], byRole: [], byModel: [], total: 0 };
}

import { errorOfEnvelope, isObject, type CpdError } from "./cpd.ts";

/** contract.md §5.2: every Event kind. A line with any other kind is not a valid Event. */
export const KINDS = [
  "context-created", "context-record", "context-grounded",
  "plan-created", "plan-phase", "plan-review", "plan-done",
  "job-added", "job-edited", "job-merged", "job-failed", "job-retried", "job-cancel-requested", "job-cancelled",
  "attempt-started", "attempt-status", "attempt-retry", "attempt-nudge",
  "all-merged", "pr-feedback",
  "escalation", "escalation-answered", "escalation-expired",
  "session-registered", "session-state", "session-moved", "session-renamed",
  "context-renamed", "plan-renamed", "context-archived", "plan-archived", "session-archived", "session-message",
  "pr-added", "pr-edited", "pr-cancelled", "pr-started", "pr-opened", "pr-merged",
  "watch-started", "watch-stopped", "error",
] as const;
export type Kind = (typeof KINDS)[number];

export type CpdEvent = {
  id: number;
  at: string;
  kind: Kind;
  context: string | null;
  plan: string | null;
  job: string | null;
  attempt: string | null;
  session: string | null;
  text: string;
  data: Record<string, unknown>;
};

/** The `data` fields the adapter reads, per kind: a line without them is invalid, so no handler checks again. */
const NEEDED: Partial<Record<Kind, readonly string[]>> = {
  escalation: ["escalation"],
  "escalation-answered": ["escalation"],
  "escalation-expired": ["escalation"],
  "plan-review": ["review", "status"],
  "session-renamed": ["name"],
  "attempt-nudge": ["prompt"],
  "session-message": ["text"],
};

const NULLABLE = ["context", "plan", "job", "attempt", "session"] as const;

export type Line =
  | { type: "event"; event: CpdEvent }
  | { type: "failure"; error: CpdError }
  | { type: "invalid"; reason: string; id?: number };

/** Parse one stdout line of `events follow` at the boundary (commands.md §11.2): an Event, the failure envelope, or invalid. */
export function parseLine(text: string): Line {
  let json: unknown;
  try {
    json = JSON.parse(text);
  } catch {
    return { type: "invalid", reason: `not JSON: ${text.slice(0, 200)}` };
  }
  if (!isObject(json)) return { type: "invalid", reason: "not a JSON object" };
  if (json.ok === false) return { type: "failure", error: errorOfEnvelope(json) };
  const id = Number.isSafeInteger(json.id) && (json.id as number) > 0 ? (json.id as number) : undefined;
  const bad = (reason: string): Line => ({ type: "invalid", reason, ...(id !== undefined ? { id } : {}) });
  if (id === undefined) return bad("`id` is not a positive integer");
  if (typeof json.at !== "string") return bad("`at` is not a string");
  if (typeof json.kind !== "string" || !(KINDS as readonly string[]).includes(json.kind)) return bad(`unknown kind ${JSON.stringify(json.kind)}`);
  const kind = json.kind as Kind;
  for (const f of NULLABLE) if (json[f] !== null && typeof json[f] !== "string") return bad(`\`${f}\` is neither a string nor null`);
  if (typeof json.text !== "string") return bad("`text` is not a string");
  if (!isObject(json.data)) return bad("`data` is not an object");
  for (const f of NEEDED[kind] ?? []) if (typeof json.data[f] !== "string") return bad(`${kind}: \`data.${f}\` is not a string`);
  if (kind === "session-message" && json.data.from !== null && typeof json.data.from !== "string") return bad("session-message: `data.from` is neither a string nor null");
  return { type: "event", event: json as unknown as CpdEvent };
}

import { CpdError, errorOfEnvelope, isObject } from "./cpd.ts";

/**
 * The adapter/cpd protocol this adapter speaks (commands.md §1.2, §15.2). Equal to `PROTOCOL` in `core/src/events/kind.rs`:
 * `pi/test/protocol.test.ts` fails otherwise. Raise both whenever an Event kind, an Event `data` field the adapter reads,
 * or the output of a command the adapter runs changes in a way the adapter depends on.
 * 9 is `session-retiered`, 10 is `plan-outlined`, 12 is `session-resumed`, 14 is `incident-started` and `incident-ended`, 15 is `mcp-attached`, 16 is the `session switch-failed` and `session switched` commands, 17 is `pr-described`, 18 is `session-model-changed`, 19 is `context pending`'s plan-level `jobs`, 20 is `attempt-aborted`.
 */
export const PROTOCOL = 20;

/** contract.md §5.2: every Event kind. A line with any other kind is not a valid Event. */
export const KINDS = [
  "context-created", "context-record", "context-grounded",
  "plan-created", "plan-phase", "plan-review", "plan-outlined", "plan-done",
  "job-added", "job-edited", "job-merged", "job-failed", "job-retried", "job-cancel-requested", "job-cancelled",
  "attempt-started", "attempt-status", "attempt-retry", "attempt-nudge", "attempt-aborted",
  "all-merged", "pr-feedback",
  "escalation", "escalation-answered", "escalation-expired",
  "session-registered", "session-state", "session-moved", "session-renamed", "session-titled", "session-retiered", "session-model-changed",
  "context-renamed", "context-owner", "plan-renamed", "context-archived", "plan-archived", "session-archived", "session-asleep", "session-resumed", "plan-deleted", "context-deleted", "records-retired", "docs-decided", "context-curated", "session-message",
  "inbox-opened", "inbox-closed", "inbox-snoozed", "inbox-dismissed", "inbox-claimed",
  "pr-added", "pr-edited", "pr-described", "pr-cancelled", "pr-started", "pr-base-changed", "pr-opened", "pr-merged",
  "watch-started", "watch-stopped", "review-opened", "review-closed", "incident-started", "incident-plans", "incident-ended", "error", "mcp-attached",
] as const;
export type Kind = (typeof KINDS)[number];

/** The `data` fields the adapter reads, per kind: `parseLine` rejects a line without them, so a handler reads them typed, with no cast. */
type Needed = {
  escalation: { escalation: string };
  "escalation-answered": { escalation: string };
  "escalation-expired": { escalation: string };
  "plan-review": { review: string; status: string };
  "session-renamed": { name: string };
  "attempt-nudge": { prompt: string };
  "session-message": { text: string; from: string | null };
  "mcp-attached": { name: string; server: Record<string, unknown> };
};

type Base = {
  id: number;
  at: string;
  context: string | null;
  plan: string | null;
  job: string | null;
  attempt: string | null;
  session: string | null;
  text: string;
};

/** An Event, per kind: `ev.kind === "session-renamed"` narrows `ev.data.name` to a string. */
export type CpdEvent = { [K in Kind]: Base & { kind: K; data: Record<string, unknown> & (K extends keyof Needed ? Needed[K] : unknown) } }[Kind];
export type EventOf<K extends Kind> = Extract<CpdEvent, { kind: K }>;

/** The checked fields, one list per kind, pinned to {@link Needed}: a field added to the type and not here (or the reverse) does not compile. */
const NEEDED: { [K in keyof Needed]: readonly (keyof Needed[K] & string)[] } = {
  escalation: ["escalation"],
  "escalation-answered": ["escalation"],
  "escalation-expired": ["escalation"],
  "plan-review": ["review", "status"],
  "session-renamed": ["name"],
  "attempt-nudge": ["prompt"],
  "session-message": ["text"],
  "mcp-attached": ["name"],
};

const NULLABLE = ["context", "plan", "job", "attempt", "session"] as const;

export type Line =
  | { type: "event"; event: CpdEvent }
  | { type: "failure"; error: CpdError }
  | { type: "unknown-kind"; id: number }
  | { type: "invalid"; reason: string; id?: number };

/** Parse one stdout line of `events follow` at the boundary (commands.md §11.2): an Event, the failure envelope, a well-formed Event of an unknown kind, or invalid. */
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
  if (typeof json.kind !== "string") return bad("`kind` is not a string");
  for (const f of NULLABLE) if (json[f] !== null && typeof json[f] !== "string") return bad(`\`${f}\` is neither a string nor null`);
  if (typeof json.text !== "string") return bad("`text` is not a string");
  if (!isObject(json.data)) return bad("`data` is not an object");
  if (!(KINDS as readonly string[]).includes(json.kind)) return { type: "unknown-kind", id }; // well-formed, but not a kind this adapter acts on
  const kind = json.kind as Kind;
  for (const f of (NEEDED as Partial<Record<string, readonly string[]>>)[kind] ?? []) if (typeof json.data[f] !== "string") return bad(`${kind}: \`data.${f}\` is not a string`);
  if (kind === "mcp-attached" && !isObject(json.data.server)) return bad("mcp-attached: `data.server` is not an object");
  if (kind === "session-message" && json.data.from !== null && typeof json.data.from !== "string") return bad("session-message: `data.from` is neither a string nor null");
  return { type: "event", event: json as unknown as CpdEvent };
}

/** `cpd events list --kind mcp-attached` at the boundary: every entry is a valid `mcp-attached` Event, or the whole read fails (commands.md §15.2 step 5a). */
export function parseMcpEvents(out: Record<string, unknown>): EventOf<"mcp-attached">[] {
  if (!Array.isArray(out.events)) throw new CpdError("bad_output", "cpd events list: `events` is not a list");
  return out.events.map((e) => {
    const line = parseLine(JSON.stringify(e));
    if (line.type !== "event" || line.event.kind !== "mcp-attached") throw new CpdError("bad_output", "cpd events list: an entry is not a valid mcp-attached Event");
    return line.event;
  });
}

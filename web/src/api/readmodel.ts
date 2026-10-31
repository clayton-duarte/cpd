// The read-model mirror of the CLI's documented JSON (docs/research/web-client-api.md §3),
// flagged to supersede the W1 core `Serialize` output (hs-d771): the mirror's shapes are the
// same names/shapes a CLI command already prints. The acceptance guard below throws until the
// impl Job lands — this is what makes the parity test RED.
//
// Source shapes: docs/commands.md §4.2 (plan list), §4.3 (plan show), §10.1 (session list),
// §21.1 (inbox list); the item shape per core/src/inbox/cmd.rs:121-149.
// MARKER: these mirror W1 Serialize output and are replaced when the shared read model lands.

/** `plan list` (§4.2). */
export interface PlanListResult {
  ok: true;
  plans: readonly PlanSummary[];
  events_cursor: string;
}

export interface PlanSummary {
  id: string;
  context: string;
  title: string;
  phase: string;
  lead_session: string | null;
  archived_at: string | null;
  counts: {
    draft: number;
    queued: number;
    running: number;
    merged: number;
    failed: number;
    cancelled: number;
  };
  prs: readonly unknown[];
}

/** `plan show` (§4.3). */
export interface PlanShowResult {
  ok: true;
  plan: {
    id: string;
    context: string;
    title: string;
    jira: unknown;
    phase: string;
    lead_session: string | null;
    created_at: string;
    locked_at: string | null;
    done_at: string | null;
    archived_at: string | null;
    outline: unknown;
  };
  prs: readonly unknown[];
  jobs: readonly unknown[];
  escalations: unknown;
  review: unknown;
  counts: unknown;
  tamper: unknown;
  events_cursor: string;
}

/** `session list` (§10.1). */
export interface SessionListResult {
  ok: true;
  sessions: readonly SessionSummary[];
  events_cursor: string;
}

export interface SessionSummary {
  id: string;
  role: string;
  context: string;
  plan: string | null;
  attempt: string | null;
  name: string | null;
  runner: string | null;
  state: string;
  state_label: string;
  state_at: string | null;
  archived_at: string | null;
  asleep_at: string | null;
  place: string | null;
  tmux_pane: string | null;
  file: string | null;
  forked_from: string | null;
  plan_lead: string | null;
  tier: string | null;
  cwd: string | null;
  title: string | null;
  titled_at: string | null;
}

/** `inbox list` (§21.1; item per core/src/inbox/cmd.rs:121-149). */
export interface InboxListResult {
  ok: true;
  engine: { running: boolean };
  items: readonly InboxItem[];
  hidden: { snoozed: number; dismissed: number; claimed: number };
  events_cursor: string;
}

export interface InboxItem {
  id: string | null;
  kind: string;
  urgency: string;
  label: string;
  subject: string;
  context: string | null;
  plan: string | null;
  job: string | null;
  session: string | null;
  record: string | null;
  event: string | null;
  url: string | null;
  opened_at: string | null;
  closed_at: string | null;
  snoozed_until: string | null;
  dismissed_at: string | null;
  claimed_at: string | null;
  age_seconds: number;
}

/** The one mirror type of the pinned CLI JSON — the two keys every read result carries. */
export type ReadModelRoot =
  | PlanListResult
  | PlanShowResult
  | SessionListResult
  | InboxListResult;

/**
 * Guard: asserts `value` parses to the read-model mirror and returns it (STUB). The parity
 * skeleton feeds the pinned CLI JSON fixture here; it throws `not implemented` until the
 * impl Job lands — this is what makes the parity test RED.
 */
export function acceptReadModel<T extends ReadModelRoot>(value: unknown): T {
  throw new Error("not implemented");
}

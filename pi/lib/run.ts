import { CpdError, isObject } from "./cpd.ts";
import { Follower, type FollowerHooks, type Schedule, type Spawn, type Timing } from "./follower.ts";
import type { SkillsNotice } from "./skills.ts";
import type { ReviewEntry } from "./plan-review.ts";
import type { Poll } from "./reload.ts";
import type { Facts, Reporter } from "./state.ts";

export type SessionInfo = { role: string; context: string; plan: string | null; planLead: boolean; registeredEvent: number; name: string | null; key: string | null; byUser: boolean; loopGuardTurns: number };

/** `cpd session show` at the boundary (§15.2 step 1): the fields the adapter reads, checked once. */
export function parseSession(show: Record<string, unknown>): SessionInfo {
  const s = show.session;
  const lacks = () => new CpdError("bad_output", "cpd session show: the session object lacks role, context, plan, plan_lead, registered_event, name or key");
  if (!isObject(s) || typeof s.role !== "string" || typeof s.context !== "string" || typeof s.plan_lead !== "boolean" || (s.plan !== null && typeof s.plan !== "string") || (s.name !== null && typeof s.name !== "string") || (s.key !== null && typeof s.key !== "string")) {
    throw lacks();
  }
  const limit = s.loop_guard_turns;
  if (typeof limit !== "number" || !Number.isSafeInteger(limit) || limit < 1) throw new CpdError("bad_output", "cpd session show: loop_guard_turns is not a positive integer");
  const registered = s.registered_event;
  if (typeof registered !== "number" || !Number.isSafeInteger(registered)) throw lacks();
  if (s.plan_lead && typeof s.plan !== "string") throw new CpdError("bad_output", "cpd session show: a Plan Lead without a plan");
  return { role: s.role, context: s.context, plan: s.plan, planLead: s.plan_lead, registeredEvent: registered, name: s.name, key: s.key, byUser: s.by_user === true, loopGuardTurns: limit };
}

/** One activation of the adapter: from `session_start` to `session_shutdown`. */
export class Run {
  /** Highest Event id handled; the next `--since`. */
  cursor = 0;
  /** Event ids delivered to pi in this activation (§15.3): an id is delivered at most once. */
  readonly delivered = new Set<number>();
  /** Ids of the own `mcp-attached` Events the start-up replay read (§15.2 step 5a): the follower's later delivery of one registers nothing. */
  readonly replayed = new Set<number>();
  reviews = new Map<string, ReviewEntry>();
  opening = new Set<string>();
  deciding = new Set<string>();
  /** Error codes already delivered since the last valid Event (commands.md §15.2, "The follower ends"). */
  followerReported = new Set<string>();
  /** §15.1 "Errors": the `what` and `code` of the last adapter error shown; an identical one is not shown again until another has been. */
  lastError: string | undefined = undefined;
  failureLineThisRun = false;
  /** `installedIdentity()` when the current follower process started. */
  startedWith: string | undefined;
  /** Events are handled one after the other, in id order, though a handler may wait for `cpd`. */
  tail: Promise<void> = Promise.resolve();
  /** True while a failed read of Context updates is reported and no read has succeeded since (§15.7 "Failure"). */
  contextFailed = false;
  /** The name last known to be the Session's, so `session_info_changed` can tell a User's rename from ours. */
  knownName: string | null;
  /** False until the start-up's `session state idle` is done: facts are kept, nothing is reported before. */
  reporting = false;
  /** Watches the adapter directory for an install (§15 "Reload after an install"). */
  reload: Poll | undefined;
  /** §15.3 `session-retiered`: the Session's Tier changed and pi's model is not yet switched; `retiering` while a switch is being made. */
  retierDue = false;
  retiering = false;
  /** §15.8: the last run ended on a message that spoke of further work and named no registered key; the next `before_agent_start` adds the reminder once. */
  reminderDue = false;
  /** The text of the `cpd-policy` section (§15.10), or undefined: none, or `cpd policy show` failed. */
  policy: string | undefined = undefined;
  /** True once the missing-`sections` failure is reported, so it is reported once per run (§8.3 "Delivery to agents"). */
  sectionsReported = false;
  /** §15.7a: a by-user Lead or Manager whose first message is not yet delivered: no turn starts before the User's first prompt. */
  firstPending = false;
  /** True while a failed read of the first message is reported and none has succeeded since. */
  firstFailed = false;
  /** The Event id of the first message delivered by this run: the same run's `cpd-context` update counts it before pi has the message in the session file. */
  firstEvent = 0;
  /** §15.15 L3: the start times of the self-started turns of the last 600 000 ms. */
  turnStarts: number[] = [];
  /** §15.15 L4: the loop guard has tripped: a send that would start a turn is queued for the next run instead. */
  tripped = false;
  /** §15.15 L8: the consecutive read-only tool calls of this turn; reset at `before_agent_start` and `agent_end`. */
  readStreak = 0;
  /** §15.15 L8: the tool calls of this turn; reset at `before_agent_start` and `agent_end`. */
  totalCalls = 0;
  /** §15.15 L8: the calls refused in this turn; at 3 the breaker trips; reset at `before_agent_start` and `agent_end`. */
  turnRefusals = 0;
  /** commands.md §15.26: the names of every tool called in this run, nested calls too; cleared at `before_agent_start` and `agent_end`. */
  questionTools = new Set<string>();
  /** commands.md §15.26: the last message of the run that ended on a question, until the settle handles it or a newer run starts. */
  questionCandidate: { text: string } | undefined = undefined;
  /** commands.md §15.26: the nudges sent since the User's last turn; at 1 no more is sent. */
  questionNudges = 0;
  /** commands.md §15.26: the User was told the classifier is unreachable and no run has reached it since. */
  classifierDown = false;
  /** commands.md §15.23: the skill files changed since the baseline; told at the next turn, then cleared. */
  skillsNotice: SkillsNotice | undefined = undefined;
  unsubscribe: (() => void)[] = [];
  readonly id: string;
  readonly info: SessionInfo;
  readonly facts: Facts;
  readonly reporter: Reporter;
  readonly abort: AbortController;
  /** One line of `run/adapter/<session>.log` (§15.6 "Log"); a failed write is notified once per run. */
  readonly log: (line: string) => void;
  readonly follower: Follower;

  constructor(init: { id: string; info: SessionInfo; facts: Facts; reporter: Reporter; abort: AbortController; log: (line: string) => void; follow: { hooks: (run: Run) => FollowerHooks; timing: Timing; spawn?: Spawn; schedule?: Schedule } }) {
    this.id = init.id;
    this.info = init.info;
    this.facts = init.facts;
    this.reporter = init.reporter;
    this.abort = init.abort;
    this.log = init.log;
    this.knownName = init.info.name;
    const { hooks, timing, spawn, schedule } = init.follow;
    this.follower = new Follower(init.id, hooks(this), timing, spawn, schedule);
  }

  /** The Plan of a Plan Lead's run. */
  get plan(): string {
    if (this.info.plan === null) throw new CpdError("not_a_lead", "this session has no Plan");
    return this.info.plan;
  }
}

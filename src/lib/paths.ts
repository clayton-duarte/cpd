import { createHash } from "node:crypto";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { usage } from "./errors.ts";

// --- identifiers (contract §3) ---

const WORKFLOW_ID = /^[a-z][a-z0-9-]{1,23}$/;
const JOB_KEY = /^[a-z][a-z0-9-]{0,31}$/;
const ATTEMPT_ID = /^[a-z][a-z0-9-]{0,31}-a[1-9][0-9]*$/;
const ESCALATION_ID = /^[a-z][a-z0-9-]{0,31}-a[1-9][0-9]*-q[1-9][0-9]*$/;
/** A lead session id becomes a directory name, so it must be a safe single path segment. */
const SESSION_ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/;

function assertMatches(re: RegExp, code: string, what: string, v: unknown): asserts v is string {
  if (typeof v !== "string" || !re.test(v)) {
    throw usage(code, `invalid ${what}: ${JSON.stringify(v)} (must match ${re.source})`, { value: v });
  }
}

export function assertWorkflowId(v: unknown): asserts v is string {
  assertMatches(WORKFLOW_ID, "invalid_workflow_id", "workflow id", v);
}
export function assertJobKey(v: unknown): asserts v is string {
  assertMatches(JOB_KEY, "invalid_job_key", "job key", v);
}
export function assertAttemptId(v: unknown): asserts v is string {
  assertMatches(ATTEMPT_ID, "invalid_attempt_id", "attempt id", v);
}
export function assertEscalationId(v: unknown): asserts v is string {
  assertMatches(ESCALATION_ID, "invalid_escalation_id", "escalation id", v);
}
export function assertSessionId(v: unknown): asserts v is string {
  assertMatches(SESSION_ID, "invalid_session_id", "lead session id", v);
}

function assertPositiveInt(code: string, what: string, n: unknown): asserts n is number {
  if (typeof n !== "number" || !Number.isInteger(n) || n < 1) {
    throw usage(code, `invalid ${what}: ${String(n)} (must be an integer >= 1)`, { value: n });
  }
}

export function attemptId(job: string, n: number): string {
  assertJobKey(job);
  assertPositiveInt("invalid_attempt_number", "attempt number", n);
  const id = `${job}-a${n}`;
  assertAttemptId(id);
  return id;
}

export function escalationId(attempt: string, k: number): string {
  assertAttemptId(attempt);
  assertPositiveInt("invalid_escalation_number", "escalation number", k);
  return `${attempt}-q${k}`;
}

export function leadBranch(wf: string): string {
  assertWorkflowId(wf);
  return `cpd/${wf}`;
}

export function attemptBranch(wf: string, attempt: string): string {
  assertAttemptId(attempt);
  return `${leadBranch(wf)}--${attempt}`;
}

export type AgentRole = "w" | "r";

/** herdr agent name: `c` + first 12 hex chars of sha1("<wf>/<attempt-id>/<role>"). */
export function agentName(wf: string, attempt: string, role: AgentRole): string {
  assertWorkflowId(wf);
  assertAttemptId(attempt);
  if (role !== "w" && role !== "r") throw usage("invalid_agent_role", `invalid agent role: ${String(role)}`, { role });
  return "c" + createHash("sha1").update(`${wf}/${attempt}/${role}`).digest("hex").slice(0, 12);
}

// --- state layout (contract §4) ---

/** `$CPD_HOME`, default `~/.cpd`. Read from the environment on every call. */
export function cpdHome(): string {
  const v = process.env.CPD_HOME;
  return v ? resolve(v) : join(homedir(), ".cpd");
}

export const configPath = () => join(cpdHome(), "config.json");

export const slotsDir = () => join(cpdHome(), "slots");
export const slotPath = (i: number) => {
  if (!Number.isInteger(i) || i < 0) throw usage("invalid_slot_index", `invalid slot index: ${String(i)}`, { index: i });
  return join(slotsDir(), `slot-${i}.json`);
};

export const locksDir = () => join(cpdHome(), "locks");
export const slotsLockPath = () => join(locksDir(), "slots.lock");
/** Per-repo merge lock, keyed by the sha1 of the repo's realpath (the caller passes the realpath). */
export const mergeLockPath = (repoRealpath: string) =>
  join(locksDir(), `merge-${createHash("sha1").update(repoRealpath).digest("hex")}.lock`);

export const leadsDir = () => join(cpdHome(), "leads");
export const leadDir = (sessionId: string) => {
  assertSessionId(sessionId);
  return join(leadsDir(), sessionId);
};
export const leadQueueDir = (sessionId: string) => join(leadDir(sessionId), "queue");
export const leadQueueRejectedDir = (sessionId: string) => join(leadQueueDir(sessionId), "rejected");
export const leadWorkflowPath = (sessionId: string) => join(leadDir(sessionId), "workflow");

export const workflowsDir = () => join(cpdHome(), "workflows");
export const workflowDir = (wf: string) => {
  assertWorkflowId(wf);
  return join(workflowsDir(), wf);
};
export const workflowJsonPath = (wf: string) => join(workflowDir(wf), "workflow.json");
export const ledgerDir = (wf: string) => join(workflowDir(wf), "ledger");
export const beadsDir = (wf: string) => join(ledgerDir(wf), ".beads");
export const fingerprintsPath = (wf: string) => join(workflowDir(wf), "fingerprints.json");
export const planPath = (wf: string) => join(workflowDir(wf), "plan.md");
export const prStatePath = (wf: string) => join(workflowDir(wf), "pr-state.json");
export const watchPath = (wf: string) => join(workflowDir(wf), "watch.json");
export const plannotatorDir = (wf: string) => join(workflowDir(wf), "plannotator");

export const escalationsDir = (wf: string) => join(workflowDir(wf), "escalations");
export const escalationQuestionPath = (wf: string, escId: string) => {
  assertEscalationId(escId);
  return join(escalationsDir(wf), `${escId}.q.json`);
};
export const escalationAnswerPath = (wf: string, escId: string) => {
  assertEscalationId(escId);
  return join(escalationsDir(wf), `${escId}.a.json`);
};

export const specsDir = (wf: string) => join(workflowDir(wf), "specs");
export const specPath = (wf: string, attempt: string) => {
  assertAttemptId(attempt);
  return join(specsDir(wf), `${attempt}.md`);
};

export const reviewsDir = (wf: string) => join(workflowDir(wf), "reviews");
const reviewFile = (wf: string, attempt: string, ext: string) => {
  assertAttemptId(attempt);
  return join(reviewsDir(wf), `${attempt}.${ext}`);
};
export const reviewBriefPath = (wf: string, attempt: string) => reviewFile(wf, attempt, "md");
export const reviewDiffPath = (wf: string, attempt: string) => reviewFile(wf, attempt, "diff");
export const reviewGatePath = (wf: string, attempt: string) => reviewFile(wf, attempt, "gate.txt");

export const wfLocksDir = (wf: string) => join(workflowDir(wf), "locks");
export const ledgerLockPath = (wf: string) => join(wfLocksDir(wf), "ledger.lock");

export const logsDir = (wf: string) => join(workflowDir(wf), "logs");
export const cpdLogPath = (wf: string) => join(logsDir(wf), "cpd.jsonl");
export const attemptLogPath = (wf: string, attempt: string) => {
  assertAttemptId(attempt);
  return join(logsDir(wf), `attempt-${attempt}.log`);
};

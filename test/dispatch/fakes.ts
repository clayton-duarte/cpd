import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import type { MergeOutcome } from "../../src/git/merge.ts";
import type { Attempt } from "../../src/ledger/attempts.ts";
import { outcomeForCause } from "../../src/ledger/attempts.ts";
import type { Job } from "../../src/ledger/jobs.ts";
import { CpdError, failure, refused } from "../../src/lib/errors.ts";
import { agentName, attemptBranch, reviewGatePath } from "../../src/lib/paths.ts";
import type { Catalog } from "../../src/lib/tiers.ts";
import type { SessionMetrics } from "../../src/metrics/session.ts";
import type { NotifyInput } from "../../src/queue/lead-queue.ts";
import { LeadMessageSchema } from "../../src/queue/messages.ts";
import type { Deps } from "../../src/dispatch/attempt-run.ts";

/** Fakes for every port of `Deps`: an in-memory Ledger that enforces the real state machine, and scripted herdr, git and Reviewer. */

export const WF = "demo";
export const CATALOG: Catalog = {
  O: { model: "p/opus", efforts: [1, 2, 3, 4, 5] },
  S: { model: "p/sonnet", efforts: [0, 1, 2, 3, 4, 5] },
  H: { model: "p/haiku", efforts: [0, 1, 2, 3] },
};
export const LEAD_SESSION = "/sessions/lead.jsonl";
export const WORKER_SESSION = "/sessions/worker.jsonl";
export const REVIEWER_SESSION = "/sessions/reviewer.jsonl";
export const TMUX_SOCKET = "/tmp/tmux-1000/cpd";
export const TMUX_REVIEWER_PANE = "%9";
export const SHA_PRE = "a".repeat(40);
export const SHA_POST = "b".repeat(40);

export type WorkerAction = "submit" | "idle" | "crash" | "timeout" | "cancel" | "tool_use";

export type Script = {
  jobKey?: string;
  attemptNumber?: number;
  jobTier?: string;
  jobMetadata?: Record<string, unknown>;
  retryCap?: number;
  reviewMode?: "fork" | "fresh" | "none";
  /** Where the Lead runs. A tmux Lead's Reviewer opens in a tmux window (default herdr). */
  leadHost?: "herdr" | "tmux";
  /** tmux Lead: the Reviewer window opens but pi never finishes a turn. */
  reviewerSilent?: boolean;
  /** What the Worker does on its n-th prompt (the Spec prompt is the first, the nudge the second). */
  worker?: WorkerAction[];
  /** The text of the Reviewer's last assistant message after its n-th prompt. */
  reviewer?: string[];
  /** Results (or thrown errors) of successive merge calls; the last one repeats. */
  merge?: (MergeOutcome | Error)[];
  setupFails?: boolean;
  /** `herdr agent start` fails for this role. */
  startFails?: "worker" | "reviewer";
  /** Merge calls advance the fake clock by this much (to exercise the merge lock wait). */
  mergeTakesMs?: number;
  mergeLockWaitMs?: number;
  /** Make a port throw an unexpected (non-Cpd) error. */
  renderThrows?: boolean;
  /** `closePane` fails with herdr_pane_not_found. */
  closePaneFails?: boolean;
  /** Run when the lifecycle sleeps (what `cpd watch` does a moment after closing a pane). */
  onSleep?: (w: World) => void;
};

export const verdictText = (verdict: "approve" | "reject", feedback = "looks fine"): string =>
  `I read the diff.\n\n\`\`\`json\n${JSON.stringify({ verdict, feedback })}\n\`\`\``;

type Mutable = {
  status: Attempt["status"];
  jobStatus: Job["status"];
  outcome: string | null;
  cause: string | null;
  submittedAt: string | null;
  metadata: Record<string, unknown>;
};

export type World = {
  deps: Deps;
  /** Ordered record of every port call that changes something. */
  events: string[];
  messages: NotifyInput[];
  closedPanes: string[];
  closedWorkspaces: string[];
  agentStarts: { name: string; args: string[] }[];
  prompts: { name: string; text: string }[];
  /** Every tmux call as `tmux -S <socket> <args>` minus the `-S <socket>`, in order. */
  tmuxCalls: { socket: string; args: string[] }[];
  metricsCalls: { path: string; since: number | undefined }[];
  mergeCalls: number;
  removed: string[];
  slotReleases: number;
  logs: string[];
  state: Mutable;
  clock: { t: number; advance(ms: number): void };
  /** Ledger calls only, in order. */
  ledger: string[];
};

export const workerName = (wf = WF, id = "api-a1") => agentName(wf, id, "w");
export const reviewerName = (wf = WF, id = "api-a1") => agentName(wf, id, "r");

export function makeWorld(script: Script = {}): World {
  const key = script.jobKey ?? "api";
  const number = script.attemptNumber ?? 1;
  const id = `${key}-a${number}`;
  const jobTier = script.jobTier ?? "S3";
  const worker = [...(script.worker ?? ["submit"])];
  const reviewer = [...(script.reviewer ?? [verdictText("approve")])];
  const merges = [...(script.merge ?? [{ result: "merged", preSha: SHA_PRE, postSha: SHA_POST, detail: "merged" } as MergeOutcome])];
  const clock = { t: Date.parse("2026-01-01T00:00:00Z"), advance(ms: number) { this.t += ms; } };
  const w: World = {
    deps: undefined as never,
    events: [],
    messages: [],
    closedPanes: [],
    closedWorkspaces: [],
    agentStarts: [],
    prompts: [],
    tmuxCalls: [],
    metricsCalls: [],
    mergeCalls: 0,
    removed: [],
    slotReleases: 0,
    logs: [],
    ledger: [],
    clock,
    state: { status: "in_progress", jobStatus: "running", outcome: null, cause: null, submittedAt: null, metadata: { number, tier: "S3" } },
  };
  const s = w.state;
  const ev = (e: string) => w.events.push(e);
  const led = (e: string) => {
    w.ledger.push(e);
    ev(`ledger.${e}`);
  };
  let workerAlive = true;
  /** When the tmux Reviewer's newest assistant message was written; null before its first one. */
  let reviewerWrittenAt: number | null = null;
  let reviewerFindPolls = 0;
  const reviewerReplies = () => {
    if (script.reviewerSilent) return;
    last[REVIEWER_SESSION] = { stopReason: "stop", text: reviewer.shift() ?? "" };
    reviewerWrittenAt = clock.t;
  };
  const last: Record<string, { stopReason: string; text: string } | null> = { [WORKER_SESSION]: null, [REVIEWER_SESSION]: null };

  const attemptView = (n: number, current: boolean): Attempt => {
    const m = current ? s.metadata : { number: n, tier: "S3", outcome: "failed", cause: "crashed" };
    const str = (k: string) => (typeof m[k] === "string" ? (m[k] as string) : null);
    return {
      id: `${key}-a${n}`,
      issue: `bd-a${n}`,
      job: key,
      jobIssue: "bd-job",
      number: n,
      tier: String(m.tier),
      status: current ? s.status : "closed",
      outcome: current ? s.outcome : "failed",
      cause: current ? s.cause : "crashed",
      worktree: str("worktree"),
      branch: str("branch"),
      agent: str("agent"),
      sessionPath: str("session_path"),
      startedAt: null,
      submittedAt: current ? s.submittedAt : null,
      endedAt: null,
      closeReason: null,
      metadata: m,
    };
  };
  const jobView = (): Job => ({
    id: "bd-job",
    key,
    title: "Api",
    status: s.jobStatus,
    tier: jobTier,
    reviewTier: "S4",
    goal: "g",
    context: "c",
    touches: ["src/a.ts"],
    acceptance: ["a"],
    outOfScope: [],
    needs: [],
    closeReason: null,
    metadata: { key, tier: jobTier, ...(script.jobMetadata ?? {}) },
  });
  const illegal = (what: string) =>
    refused("illegal_transition", `fake ledger: cannot ${what} (attempt ${s.status}, job ${s.jobStatus})`);
  const once = (k: string) => {
    if (k in s.metadata) throw refused("metadata_write_once", `fake ledger: ${k} already set`);
  };

  w.deps = {
    wf: WF,
    attemptId: id,
    config: { retryCap: script.retryCap ?? 2, reviewMode: script.reviewMode ?? "fork", attemptTimeoutMinutes: 30 },
    catalog: CATALOG,
    workflow: {
      repo: "/repo",
      leadWorktree: "/repo.lead",
      leadBranch: `cpd/${WF}`,
      lead: script.leadHost === "tmux" ? { sessionPath: LEAD_SESSION, host: "tmux", tmuxSocket: TMUX_SOCKET } : { sessionPath: LEAD_SESSION, host: "herdr" },
    },
    ledger: {
      async load() {
        const siblings = Array.from({ length: number }, (_, i) => attemptView(i + 1, i + 1 === number));
        return { attempt: attemptView(number, true), job: jobView(), siblings };
      },
      async verifyJob() {
        led("verifyJob");
      },
      async recordFacts(facts) {
        for (const k of Object.keys(facts)) once(k);
        Object.assign(s.metadata, facts);
        led(`recordFacts ${Object.keys(facts).join(",")}`);
      },
      async recordVerdict(v) {
        if (s.status !== "in_review") throw illegal("record a verdict");
        if (s.outcome !== null) throw refused("metadata_write_once", "fake ledger: verdict already recorded");
        s.outcome = v.verdict === "approve" ? "approved" : "rejected";
        led(`recordVerdict ${v.verdict}`);
      },
      async fail(cause, opts) {
        const ok = s.status === "in_progress" ? s.jobStatus === "running" : s.status === "in_review" && s.jobStatus === "in_review";
        if (!ok) throw illegal("fail");
        s.outcome = outcomeForCause(cause);
        s.cause = cause;
        s.status = "closed";
        s.jobStatus = opts.requeue ? "queued" : "failed";
        led(`fail ${cause} requeue=${opts.requeue}`);
      },
      async markMerged(sha, opts) {
        if (s.status !== "in_review" || s.jobStatus !== "in_review") throw illegal("mark merged");
        if (opts.reviewed ? s.outcome !== "approved" : s.outcome !== null) throw illegal("mark merged");
        if (!opts.reviewed) s.outcome = "merged";
        s.status = "closed";
        s.jobStatus = "closed";
        led(`markMerged ${sha}`);
      },
      async recordMetrics(m, prefix) {
        once(`${prefix}_turns`);
        s.metadata[`${prefix}_turns`] = m.turns;
        led(`recordMetrics ${prefix}`);
      },
    },
    git: {
      async createWorktree(branch) {
        ev(`git.createWorktree ${branch}`);
        if (script.setupFails) throw failure("setup_failed", "attempt worktree setup failed (exit 3)", { stderrTail: "boom", cleanup: { ok: true } });
        return { path: `/wt/${id}` };
      },
      async removeWorktree(branch) {
        w.removed.push(branch);
        ev(`git.removeWorktree ${branch}`);
      },
      async diff(branch) {
        ev(`git.diff ${branch}`);
        return "diff --git a/x b/x\n+hello\n";
      },
      async merge(path, branch) {
        w.mergeCalls++;
        ev(`git.merge ${branch}`);
        if (script.mergeTakesMs) clock.advance(script.mergeTakesMs);
        const next = merges.length > 1 ? merges.shift()! : merges[0]!;
        if (next instanceof Error) throw next;
        return next;
      },
    },
    herdr: {
      async openWorktree(o) {
        ev(`herdr.openWorktree ${o.label}`);
        return "ws1";
      },
      async rootPane() {
        return "pane-w";
      },
      async split(target, cwd) {
        ev(`herdr.split ${target} ${cwd}`);
        if (script.startFails === "reviewer") throw failure("herdr_pane_split_failed", "cannot split");
        return "pane-r";
      },
      async startAgent(o) {
        w.agentStarts.push({ name: o.name, args: o.args });
        ev(`herdr.startAgent ${o.name}`);
        const isWorker = o.name === workerName(WF, id);
        if ((isWorker && script.startFails === "worker") || (!isWorker && script.startFails === "reviewer")) {
          throw failure("herdr_agent_not_ready", "agent never became ready");
        }
        return { argv: ["pi", ...o.args], sessionPath: isWorker ? WORKER_SESSION : REVIEWER_SESSION };
      },
      async promptAgent(name, text) {
        w.prompts.push({ name, text });
        ev(`herdr.prompt ${name === workerName(WF, id) ? "worker" : "reviewer"}`);
        if (name === workerName(WF, id)) {
          const action = worker.shift() ?? "idle";
          last[WORKER_SESSION] = { stopReason: "stop", text: "ok" };
          if (action === "submit") {
            mkdirSync(dirname(reviewGatePath(WF, id)), { recursive: true });
            writeFileSync(reviewGatePath(WF, id), "verify: ok\n");
            s.status = "in_review";
            s.jobStatus = "in_review";
            s.submittedAt = "2026-01-01T00:10:00.000Z";
          } else if (action === "crash") {
            workerAlive = false;
            last[WORKER_SESSION] = { stopReason: "toolUse", text: "" };
          } else if (action === "tool_use") {
            last[WORKER_SESSION] = { stopReason: "toolUse", text: "" };
          } else if (action === "timeout") {
            workerAlive = false;
            s.metadata.timed_out = true;
          } else if (action === "cancel") {
            s.status = "closed";
            s.jobStatus = "cancelled";
            s.outcome = "cancelled";
          }
        } else {
          last[REVIEWER_SESSION] = { stopReason: "stop", text: reviewer.shift() ?? "" };
        }
        return {};
      },
      async agentGet(name) {
        if (name === workerName(WF, id)) return workerAlive ? { pane_id: "pane-w", agent_status: "idle" } : null;
        return { pane_id: "pane-r", agent_status: "idle" };
      },
      async closePane(pane) {
        ev(`herdr.closePane ${pane}`);
        if (script.closePaneFails) throw failure("herdr_pane_not_found", `no pane ${pane}`);
        w.closedPanes.push(pane);
      },
      async closeWorkspace(ws) {
        ev(`herdr.closeWorkspace ${ws}`);
        w.closedWorkspaces.push(ws);
      },
    },
    tmux: {
      async newWindow(socket, o) {
        w.tmuxCalls.push({ socket, args: ["new-window", "-d", "-n", o.name, "-c", o.cwd, o.command] });
        ev("tmux.newWindow");
        reviewerReplies(); // pi starts, reads the brief from its command line and answers
        return TMUX_REVIEWER_PANE;
      },
      async setWindowOption(socket, pane, option, value) {
        w.tmuxCalls.push({ socket, args: ["set-option", "-w", "-t", pane, option, value] });
      },
      async sendKeys(socket, pane, keys, literal = false) {
        w.tmuxCalls.push({ socket, args: ["send-keys", "-t", pane, ...(literal ? ["-l"] : []), ...keys] });
        if (keys.includes("Enter")) reviewerReplies(); // the re-prompt is submitted: pi answers it
      },
      async killWindow(socket, pane) {
        w.tmuxCalls.push({ socket, args: ["kill-window", "-t", pane] });
        ev(`tmux.killWindow ${pane}`);
      },
    },
    session: {
      findById(id) {
        void id;
        return reviewerWrittenAt !== null && ++reviewerFindPolls > 1 ? REVIEWER_SESSION : null; // pi creates the file a moment after the window opens
      },
      tail(path) {
        void path;
        return reviewerWrittenAt === null ? null : { role: "assistant", stopReason: "stop", writtenMs: reviewerWrittenAt, mtimeMs: reviewerWrittenAt };
      },
      lastAssistant(path) {
        const v = last[path];
        if (v === undefined) throw new CpdError("session_not_found", `no such session ${path}`, 4);
        return v;
      },
      cwdOf: () => "/lead/cwd",
      metrics(path, since) {
        w.metricsCalls.push({ path, since });
        return { turns: 3, tokensIn: 1, tokensOut: 2, cacheRead: 0, cacheWrite: 0, costUsd: 0.5, durationS: 10, model: "m", thinkingLevel: "low", finalStopReason: "stop", skipped: 0 } satisfies SessionMetrics;
      },
    },
    queue: {
      notify(input) {
        // Every message must be valid for the real queue.
        LeadMessageSchema.parse({ v: 1, id: "x", wf: WF, ...input, data: input.data ?? {}, createdAt: "2026-01-01T00:00:00.000Z" });
        w.messages.push(input);
        ev(`queue.${input.kind}`);
      },
    },
    render: {
      async workerSpec() {
        if (script.renderThrows) throw new Error("template exploded");
        // The real renderer needs `worktree` in the Ledger: pin the step 2 → step 3 order.
        if (typeof s.metadata.worktree !== "string") throw failure("template_missing_value", "template value is missing: worktree");
        ev("render.workerSpec");
        return `/specs/${id}.md`;
      },
      async reviewBrief(inputs) {
        ev("render.reviewBrief");
        void inputs;
        return `/reviews/${id}.md`;
      },
    },
    clock: {
      now: () => clock.t,
      async sleep(ms) {
        clock.advance(ms);
        ev(`clock.sleep ${ms}`);
        script.onSleep?.(w);
      },
    },
    slot: {
      async release() {
        w.slotReleases++;
        ev("slot.release");
      },
    },
    log: (step, message) => w.logs.push(`[${step}] ${message}`),
    ...(script.mergeLockWaitMs === undefined ? {} : { mergeLockWaitMs: script.mergeLockWaitMs }),
  };
  return w;
}

export const branchOf = (id = "api-a1") => attemptBranch(WF, id);

import { homedir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { newReporter } from "./agent-state.ts";
import { CpdError, isObject, toCpdError } from "./cpd.ts";
import { cursorFrom } from "./cursor.ts";
import { PERMANENT_CODES } from "./follower.ts";
import { boldPhi } from "./phi.ts";
import { policySection } from "./policy.ts";
import { holdTrip, trippedIn } from "./loop-guard.ts";
import { replayServers } from "./mcp.ts";
import { followerHooks } from "./handle.ts";
import type { Host } from "./host.ts";
import type { Ctx, ModelCtx } from "./pi.ts";
import { ENTRY_TYPE, highestReview, REVIEW_LABEL, reviewsFromEntries } from "./plan-review.ts";
import { registerPlanReviewCommand } from "./plan-review-command.ts";
import { adapterPath, appendLog, DEFAULT_POLL_MS, LONG_REFUSAL_NOTICE, pollAdapter, refusal, reloadLogPath, RELOAD_TRIGGER, validatePackage } from "./reload.ts";
import { guardOf } from "./reload-command.ts";
import { registerSecretTool } from "./secret.ts";
import { registerExternalApproveTool } from "./externalapprove.ts";
import { registerHostApproveTool } from "./hostapprove.ts";
import { QUESTION_LABEL, registerTool, showDialog } from "./review.ts";
import { Run, parseSession, type SessionInfo } from "./run.ts";
import { Facts } from "./state.ts";
import { firstMessageDelivered, registerFirstMessage } from "./first-message.ts";
import { checkSkills, skillOf } from "./skills.ts";
import { registerUpdates } from "./turns.ts";
import { clearMentions, refreshMentions } from "./mentions-refresh.ts";

// Start-up and shutdown (§15.2).

export async function start(host: Host, ctx: ModelCtx, reason: string): Promise<void> {
  if (ctx.mode !== "tui") return;
  host.rendering.load({ cwd: ctx.cwd, trusted: ctx.isProjectTrusted() });
  const id = host.env.CPD_SESSION;
  if (id === undefined || id === "") return;
  await stop(host); // a start without a shutdown (never seen) must not leave a second follower behind
  host.abort = new AbortController();
  host.ctx = ctx;
  if (ctx.sessionManager.getSessionId() !== id) {
    host.report("activation", "session_mismatch", `CPD_SESSION is ${id} but pi's session is ${ctx.sessionManager.getSessionId()}; the adapter stays inactive.`);
    return;
  }

  // 1. who am I
  let info: SessionInfo;
  try {
    info = await showSession(host, id);
  } catch (e) {
    const err = toCpdError(e);
    host.deliver("session show", err);
    if (PERMANENT_CODES.has(err.code)) return;
    const retry = (host.showRetry = new AbortController());
    void retryShow(host, id, retry, err.code)
      .then((shown) => (shown ? begin(host, ctx, id, shown, reason) : undefined))
      .catch((e) => host.deliver("start-up", e));
    return;
  }
  await begin(host, ctx, id, info, reason);
}

const showSession = async (host: Host, id: string) => parseSession(await host.call(["session", "show", id]));

/** §15.2 step 1 again, with the follower's back-off, until it works, is refused for good, or the run is stopped (then `undefined`). A code is reported once. */
async function retryShow(host: Host, id: string, retry: AbortController, reported: string): Promise<SessionInfo | undefined> {
  const seen = new Set([reported]);
  let delay = host.timing.restartInitialMs;
  for (;;) {
    await (host.options.sleep ?? sleep)(delay, undefined, { signal: retry.signal }).catch(() => undefined);
    if (retry.signal.aborted) return undefined;
    delay = Math.min(delay * 2, host.timing.restartMaxMs);
    try {
      const info = await showSession(host, id);
      if (retry.signal.aborted) return undefined;
      host.showRetry = undefined;
      return info;
    } catch (e) {
      if (retry.signal.aborted) return undefined;
      const err = toCpdError(e);
      if (!seen.has(err.code)) {
        seen.add(err.code);
        host.deliver("session show", err);
      }
      if (PERMANENT_CODES.has(err.code)) {
        host.showRetry = undefined;
        return undefined;
      }
    }
  }
}

/** `run/adapter/<session>.log` (§15.6 "Log"): a failed write is notified once per run, not as a message, so it starts no turn. */
function logTo(host: Host, id: string): (line: string) => void {
  let failed = false;
  return (line) => {
    try {
      appendLog(reloadLogPath(host.env.CPD_HOME || join(homedir(), ".cpd"), id), line);
    } catch (e) {
      if (!failed) {
        failed = true;
        host.ctx?.ui.notify(boldPhi(`φ: cannot write the reload log: ${toCpdError(e).message}`), "error");
      }
    }
  };
}

/** §15.2 steps 2 to 6, once the Session is known. */
async function begin(host: Host, ctx: Ctx, id: string, info: SessionInfo, reason: string): Promise<void> {
  const { pi, options } = host;
  // 1b. the policy text (§15.10): a failure is an error message and no section
  let policy: string | undefined;
  let policyFailure: unknown;
  try {
    policy = policySection(await host.call(["policy", "show"]));
  } catch (e) {
    policyFailure = e; // reported once the run exists: its role says where an error goes (§15.1)
  }
  if (host.abort.signal.aborted) return;

  const facts = new Facts();
  const r = new Run({
    id,
    info,
    facts,
    reporter: newReporter(host, id, facts),
    abort: host.abort,
    log: logTo(host, id),
    follow: { hooks: followerHooks(host), timing: host.timing, spawn: options.spawn, schedule: options.schedule },
  });
  r.policy = policy;
  host.run = r;
  r.log(`start: ${reason}`);
  if (policyFailure !== undefined) host.deliver("policy show", policyFailure);

  // 2. stopped -> idle, then working when pi is busy.
  r.reporter.want({ state: "idle" });
  await r.reporter.settled();
  if (host.run !== r) return;
  r.reporting = true;
  facts.active = !ctx.isIdle();

  // 3. name
  if (info.name !== null && pi.getSessionName() !== info.name) pi.setSessionName(info.name);

  // 4. cursor
  const entries = ctx.sessionManager.getEntries();
  r.cursor = cursorFrom(info.registeredEvent, entries, id);

  if (info.role === "manager" || info.role === "lead") {
    r.firstPending = info.byUser && !firstMessageDelivered(entries, id);
    if (r.firstPending) registerFirstMessage(host); // before registerUpdates: pi runs handlers in registration order, and the first message's Event must count before the same run's cpd-context reads its cursor
    registerUpdates(host);
  }
  void refreshMentions(host, r);

  // 5. blocking reasons (Plan Lead only; a Manager has none)
  let chatToReopen: { reviews: string[]; title: string | undefined } = { reviews: [], title: undefined };
  if (info.role !== "manager" && info.planLead && info.plan !== null) {
    const plan = info.plan;
    const folded = reviewsFromEntries(entries, plan);
    if (folded.invalid > 0) host.deliver("session entries", new CpdError("entry_invalid", `${folded.invalid} ${ENTRY_TYPE} entries of this session do not parse and were skipped`));
    r.reviews = folded.reviews;
    let latest: string | undefined = highestReview(r.reviews.keys()); // stands in when `plan show` fails (§15.5 "Latest only")
    let latestStatus: unknown; // what `plan show` says of it; unknown when it failed
    let latestTitle: string | undefined;
    try {
      const show = await host.call(["plan", "show", "--plan", plan]);
      latest = isObject(show.review) && typeof show.review.id === "string" ? show.review.id : undefined;
      latestStatus = isObject(show.review) ? show.review.status : undefined;
      latestTitle = isObject(show.review) && typeof show.review.title === "string" ? show.review.title : undefined;
      if (Array.isArray(show.escalations)) {
        for (const q of show.escalations) if (isObject(q) && typeof q.id === "string") facts.open(`escalation:${q.id}`, QUESTION_LABEL, "agent");
      }
    } catch (e) {
      host.deliver("plan show", e);
    }
    // A chat review that cpd reports decided opens nothing: its Event, replayed from the cursor, is "Deciding elsewhere" (§15.5 "Recovery").
    const decidedStatus = latestStatus === "approved" || latestStatus === "rejected" || latestStatus === "superseded";
    const shownEntry = latest === undefined ? undefined : r.reviews.get(latest);
    if (latestStatus === "superseded" && shownEntry?.status === "pending") r.reviews.set(shownEntry.review, { ...shownEntry, status: "superseded" }); // in memory: a replayed Event then changes nothing
    const reopen: string[] = [];
    for (const entry of r.reviews.values()) {
      if (entry.status !== "pending" || entry.review !== latest) continue;
      if (decidedStatus) continue;
      if (latestStatus !== undefined && latestStatus !== "pending") continue;
      reopen.push(entry.review);
      facts.open(`plan-review:${entry.review}`, REVIEW_LABEL, "human");
    }
    chatToReopen = { reviews: reopen, title: latestTitle };
    registerTool(host);
    registerSecretTool(host);
    registerHostApproveTool(host);
    registerPlanReviewCommand(host);
  }
  // Plan Leads and Managers (the Plan-Lead block above lacks the Managers): the tool cpd_external_approve
  if (info.role === "manager" || (info.role === "lead" && info.planLead)) registerExternalApproveTool(host);
  // 5a. MCP replay
  await replayServers(host, r);
  if (host.run !== r) return;
  const skill = skillOf(info.role);
  if (skill !== undefined) checkSkills(host, r, ctx.sessionManager.getBranch(), skill, join(options.reload?.dir ?? adapterPath(), "skills", skill)); // commands.md §15.23
  if (trippedIn(entries)) holdTrip(r); // §15.15 L7: a trip survives a reload; the reason and the gate are back before the follower starts
  host.refresh(r);

  // 6. follow
  r.follower.start();
  r.reload = pollAdapter({
    dir: options.reload?.dir ?? adapterPath(),
    intervalMs: options.reload?.intervalMs ?? DEFAULT_POLL_MS,
    validate: (dir) => validatePackage(dir, options.reload),
    refusal: () => (host.run === r && host.ctx !== undefined ? refusal(guardOf(r, host.ctx)) : "not active"),
    trigger: () => pi.sendUserMessage(RELOAD_TRIGGER, { expandPromptTemplates: true }),
    onRefused: (reason) => host.ctx?.ui.notify(boldPhi(`φ: adapter update not loaded: ${reason}`), "error"),
    onError: (e) => host.deliver("reload after install", e),
    log: r.log,
    onLongRefusal: () => host.ctx?.ui.notify(LONG_REFUSAL_NOTICE),
    longRefusalMs: options.reload?.longRefusalMs,
    now: options.reload?.now,
  });

  for (const review of chatToReopen.reviews) showDialog(host, r, review, chatToReopen.title);
}

export async function stop(host: Host): Promise<void> {
  host.showRetry?.abort();
  host.showRetry = undefined;
  host.abort.abort(); // also a start still waiting for `session show`
  const r = host.run;
  if (!r) return;
  host.run = undefined;
  host.ctx = undefined;
  clearMentions();
  r.log("stop");
  r.reload?.stop();
  r.follower.stop();
  r.unsubscribe.forEach((u) => u());
  if (r.reporting) {
    await Promise.race([r.reporter.close({ state: "stopped" }), (host.options.sleep ?? sleep)(host.timing.shutdownWaitMs, undefined, { ref: false })]);
  }
}

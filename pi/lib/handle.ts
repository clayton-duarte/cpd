import { CpdError, installedIdentity, isObject } from "./cpd.ts";
import { type Hold, eventMessage, FOLLOW_UP, gated, send, STEER, leadRule, sessionMessage } from "./delivery.ts";
import { type CpdEvent, type EventOf, parseLine } from "./event.ts";
import { type FollowerHooks, PERMANENT_CODES } from "./follower.ts";
import type { Host } from "./host.ts";
import { boldPhi } from "./phi.ts";
import { clearTrip } from "./loop-guard.ts";
import { checkProtocol } from "./protocol.ts";
import { closeSuperseded, decidedElsewhere, openReview, QUESTION_LABEL, supersede } from "./review.ts";
import type { McpServerConfig } from "@earendil-works/pi-coding-agent";
import { attachServer } from "./mcp.ts";
import { noteRetier } from "./retier.ts";
import type { Run } from "./run.ts";
import { refreshMentions } from "./mentions-refresh.ts";

// The follower's lines (§15.2, §15.3).

/** The hooks of `r`'s follower: the cursor, the protocol check before each process, every line, every failure. */
export const followerHooks =
  (host: Host) =>
  (r: Run): FollowerHooks => ({
    before: () => checkProtocol(host),
    cursor: () => r.cursor,
    onRunStart: () => {
      r.failureLineThisRun = false;
      r.startedWith = installedIdentity();
    },
    onLine: (line) => handleLine(host, r, line),
    onFailure: (e) => {
      if (e.code === "follower_exit" && r.failureLineThisRun) return; // the failure line already said why
      followerError(host, r, e);
    },
  });

function followerError(host: Host, r: Run, e: CpdError): void {
  if (r.followerReported.has(e.code)) return;
  r.followerReported.add(e.code);
  host.deliver("events follow", e);
}

/** Returns true when the line was a valid Event, or a `store_too_new` line of a replaced cpd (both reset the restart delay). The cursor moves past every line that has an id, delivered or not. */
function handleLine(host: Host, r: Run, line: string): boolean {
  const parsed = parseLine(line);
  if (parsed.type === "failure") {
    r.failureLineThisRun = true;
    // A newer cpd was installed since this follower started: the restart runs it, so the line is not delivered (§15.2).
    if (parsed.error.code === "store_too_new" && installedIdentity() !== r.startedWith) return true; // true: the restart delay goes back to the initial one
    followerError(host, r, parsed.error);
    if (PERMANENT_CODES.has(parsed.error.code)) r.follower.stop(); // a refusal: restarting would fail the same way (§15.2)
    return false;
  }
  if (parsed.type === "unknown-kind") {
    r.cursor = Math.max(r.cursor, parsed.id); // skipped silently: a kind this adapter does not know is one it cannot need
    return false;
  }
  if (parsed.type === "invalid") {
    host.deliver("events follow", new CpdError("invalid_event", `skipped a line that is not a valid Event: ${parsed.reason}`));
    if (parsed.id !== undefined) r.cursor = Math.max(r.cursor, parsed.id);
    return false;
  }
  r.followerReported.clear();
  const event = parsed.event;
  r.tail = r.tail.then(async () => {
    if (host.run !== r) return;
    try {
      await handleEvent(host, r, event);
    } catch (e) {
      host.deliver(`event ${event.id} (${event.kind})`, e);
    }
  });
  r.cursor = Math.max(r.cursor, event.id);
  return true;
}

// The Event kinds that change the mention ids.
export const MENTION_KINDS: ReadonlySet<string> = new Set([
  "session-registered",
  "plan-created",
  "context-created",
  "plan-deleted",
  "context-deleted",
  "session-renamed",
  "plan-renamed",
  "context-renamed",
  "session-archived",
  "plan-archived",
  "context-archived",
]);

/** §15.3: an Event id reaches pi at most once per activation; the check and the add are one synchronous step. */
function deliverOnce(host: Host, r: Run, ev: CpdEvent, message: Parameters<typeof host.pi.sendMessage>[0], options: Parameters<typeof gated>[1], hold: Hold = "all"): void {
  if (r.delivered.has(ev.id)) return;
  r.delivered.add(ev.id);
  send(host, r, message, options, hold);
}

async function handleEvent(host: Host, r: Run, ev: CpdEvent): Promise<void> {
  if (MENTION_KINDS.has(ev.kind)) void refreshMentions(host, r);
  const { pi } = host;
  const own = ev.session === r.id;
  if (ev.kind === "session-resumed") {
    // §15.15 L6: for its own Session while tripped, the trip is cleared and ONE message starts the turn that sees what queued; otherwise nothing.
    if (own && (r.info.role === "lead" || r.info.role === "manager") && clearTrip(host, r)) deliverOnce(host, r, ev, eventMessage(ev, r.id), FOLLOW_UP(true));
    return;
  }
  if (ev.kind === "session-message") {
    if (own && (r.info.role === "lead" || r.info.role === "manager")) await deliverSessionMessage(host, r, ev);
    return;
  }
  if (ev.kind === "session-retiered") {
    if (own && (r.info.role === "lead" || r.info.role === "manager")) await noteRetier(host, r);
    return;
  }
  if (ev.kind === "session-model-changed") {
    // §15.3 `session-model-changed` (the tier-preset switch, §14.6): the same silent switch path as `session-retiered`.
    if (own && (r.info.role === "lead" || r.info.role === "manager")) await noteRetier(host, r);
    return;
  }
  if (ev.kind === "mcp-attached") {
    if (own && !r.replayed.has(ev.id)) {
      try {
        attachServer(pi, ev.data.name, ev.data.server as unknown as McpServerConfig);
      } catch (e) {
        host.deliver(`mcp attach ${ev.data.name}`, e, { name: ev.data.name });
      }
    }
    return;
  }
  if (ev.kind === "session-renamed") {
    if (!own) return;
    const name = ev.data.name;
    r.knownName = name;
    pi.setSessionName(name);
    host.ctx?.ui.notify(boldPhi(`φ: session renamed to "${name}"`));
    return;
  }
  if (!r.info.planLead) {
    if (ev.kind === "attempt-nudge" && own) deliverOnce(host, r, ev, eventMessage(ev, r.id, ev.data.prompt), FOLLOW_UP(true));
    if (ev.kind === "job-addendum" && own) deliverOnce(host, r, ev, eventMessage(ev, r.id), STEER(true));
    return;
  }
  const rule = leadRule(ev);
  if (rule) {
    deliverOnce(host, r, ev, eventMessage(ev, r.id), { triggerTurn: rule.turn, deliverAs: rule.deliverAs });
    if (ev.kind === "escalation" && r.facts.open(`escalation:${ev.data.escalation}`, QUESTION_LABEL, "agent")) host.refresh(r);
    return;
  }
  if (ev.kind === "escalation-answered" || ev.kind === "escalation-expired") {
    if (r.facts.close(`escalation:${ev.data.escalation}`)) host.refresh(r);
    return;
  }
  if (ev.kind === "plan-review") {
    const review = ev.data.review;
    if (ev.data.status === "pending") {
      supersede(r, review);
      void openReview(host, r, review, "event")
        .catch((e) => host.deliver(`plan review ${review}`, e))
        .finally(() => host.refresh(r));
    }
    if (ev.data.status === "approved" || ev.data.status === "rejected") {
      const entry = r.reviews.get(review);
      if (entry?.status === "pending") void decidedElsewhere(host, r, entry, ev).catch((e) => host.deliver(`plan review ${entry.review}`, e));
    }
    if (ev.data.status === "superseded") {
      const entry = r.reviews.get(review);
      if (entry?.status === "pending") closeSuperseded(host, r, entry);
    }
  }
}

/** §15.3 `session-message`: for every Lead and a Manager; a message and a turn always, whatever `data.wake`, steered (injected after the current tool call of a busy run). The sender's name is read once, `cpd session show <from>`; the id stands in when it cannot be read. */
async function deliverSessionMessage(host: Host, r: Run, ev: EventOf<"session-message">): Promise<void> {
  const from = ev.data.from;
  let sender: { id: string; name: string | null } | null = null;
  if (from !== null) {
    sender = { id: from, name: null };
    try {
      const shown = (await host.call(["session", "show", from])).session;
      if (isObject(shown) && typeof shown.name === "string") sender.name = shown.name;
    } catch {
      // an unreadable sender is still named by its id
    }
  }
  if (host.run !== r) return;
  deliverOnce(host, r, ev, sessionMessage(ev, r.id, sender), STEER(true), "tripped"); // pro-d400, docs/commands.md §15.7a: a User message starts the first turn
}

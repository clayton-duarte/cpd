import { CpdError, isObject } from "./cpd.ts";
import type { Host } from "./host.ts";
import type { Run } from "./run.ts";

/** commands.md §15.3 `session-retiered` and `session-model-changed` (the tier-preset switch, §14.6): the Session's model
 * changed (`cpd session retier`, or `cpd preset apply` rewrote the host `[model]`). The switch waits for the Session's
 * next idle and reads the row then, so several Events before one idle make one switch. */
export async function noteRetier(host: Host, r: Run): Promise<void> {
  r.retierDue = true;
  await switchModel(host, r);
}

/** What §15.3 means by idle: pi says so, no turn is queued, and the Session is not `working`, `waiting` or `blocked`. */
function idle(host: Host, r: Run): boolean {
  const ctx = host.ctx;
  return host.run === r && ctx !== undefined && ctx.isIdle() && !ctx.hasPendingMessages() && r.facts.wanted().state === "idle";
}

/** Whether the Session is in a safe state to switch: idle, or ready before the next request (commands.md §10.16). Never while waiting or blocked. */
function canSwitch(host: Host, r: Run, beforeRequest = false): boolean {
  if (host.run !== r) return false;
  if (beforeRequest) {
    const state = r.facts.wanted().state;
    return state !== "waiting" && state !== "blocked";
  }
  return idle(host, r);
}

/** Switches pi to the row's model and thinking level when one is due:
 * 1. at idle (never while working, waiting or blocked)
 * 2. before the next provider request or retry (commands.md §10.16, never while waiting or blocked)
 * Called by the Event, every `agent_settled`, and on `agent_start` (or before_agent_start). */
export async function switchModel(host: Host, r: Run, beforeRequest = false): Promise<void> {
  if (r.retiering) return;
  r.retiering = true;
  try {
    while (r.retierDue && canSwitch(host, r, beforeRequest)) {
      r.retierDue = false;
      await apply(host, r, beforeRequest);
    }
  } finally {
    r.retiering = false;
  }
}

/** The `model` and `thinking` of `cpd session show`: the mapping from the Tier is cpd's (commands.md §10.2), the adapter reads no config. */
async function apply(host: Host, r: Run, beforeRequest = false): Promise<void> {
  let model: string;
  let thinking: string | null;
  try {
    const shown = (await host.call(["session", "show", r.id])).session;
    if (!isObject(shown) || typeof shown.model !== "string" || (shown.thinking !== null && typeof shown.thinking !== "string")) throw new CpdError("bad_output", "cpd session show: the session object lacks model or thinking");
    ({ model, thinking } = { model: shown.model, thinking: shown.thinking });
  } catch (e) {
    if (host.run === r) host.deliver("model switch", e);
    return;
  }
  if (!canSwitch(host, r, beforeRequest)) {
    r.retierDue = true; // the Session moved out of a safe switch state while the row was read: the switch is still owed
    return;
  }
  const unavailable = async (why: string) => {
    const text = `${thinking === null ? model : `${model}:${thinking}`}: ${why}`;
    host.deliver("model switch", new CpdError("model_unavailable", text), { model, thinking });
    await record(host, r, ["switch-failed", "--code", "model_unavailable", "--message", text]);
  };
  const slash = model.indexOf("/");
  const found = slash > 0 ? host.ctx?.modelRegistry.find(model.slice(0, slash), model.slice(slash + 1)) : undefined;
  if (found === undefined) return unavailable("pi has no such model");
  let accepted: boolean;
  try {
    accepted = await host.pi.setModel(found);
  } catch (e) {
    return unavailable(e instanceof Error ? e.message : String(e));
  }
  if (!accepted) return unavailable("pi has no authentication for its provider");
  if (thinking !== null) host.pi.setThinkingLevel(thinking as Parameters<Host["pi"]["setThinkingLevel"]>[0]);
  await record(host, r, ["switched"]);
}

/** `cpd session switch-failed` / `switched` (commands.md §10.18a/b): the row learns what pi did. A failed call is an adapter error, never thrown. */
async function record(host: Host, r: Run, sub: string[]): Promise<void> {
  try {
    await host.call(["session", ...sub, "--session", r.id]);
  } catch (e) {
    if (host.run === r) host.deliver("model switch", e);
  }
}

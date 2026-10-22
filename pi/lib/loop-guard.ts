import { isObject } from "./cpd.ts";
import type { Host } from "./host.ts";
import type { Run } from "./run.ts";

/** commands.md §15.15: the loop guard of a Lead or Manager. */

/** L3: the sliding window, in ms. */
export const WINDOW_MS = 600_000;
/** The pi session entry that keeps a trip across a reload (L4 (c), L7). */
export const ENTRY_TYPE = "cpd-loop-guard";
export const KEY = "loop-guard";
export const LABEL = "cpd: loop guard";

const guarded = (r: Run) => r.info.role === "lead" || r.info.role === "manager";

/** L7: the last `cpd-loop-guard` entry of the session says whether the breaker is tripped. */
export function trippedIn(entries: readonly unknown[]): boolean {
  const last = entries.findLast((e) => isObject(e) && e.type === "custom" && e.customType === ENTRY_TYPE);
  return isObject(last) && isObject(last.data) && last.data.tripped === true;
}

/** Opens the reason and the gate, without reporting: the caller refreshes. */
export function holdTrip(r: Run): void {
  r.tripped = true;
  r.facts.open(KEY, LABEL, "human");
}

/** L5, L6: the window is emptied and the trip, when there is one, is cleared (the reason closes, the entry says so, deliveries trigger turns again). True when it was tripped. */
export function clearTrip(host: Host, r: Run): boolean {
  r.turnStarts = [];
  if (!r.tripped) return false;
  r.tripped = false;
  r.facts.close(KEY);
  host.pi.appendEntry(ENTRY_TYPE, { tripped: false });
  host.refresh(r);
  return true;
}

/** L4: the breaker trips. */
function trip(host: Host, r: Run, at: number): void {
  holdTrip(r);
  host.pi.appendEntry(ENTRY_TYPE, { tripped: true, at, turns: r.turnStarts.length });
  host.refresh(r);
}

/** Registered once (pi has no unsubscribe); the run and its role are read at each call. */
export function registerLoopGuard(host: Host): void {
  host.pi.on("before_agent_start", function loopGuardHandler(event) {
    const r = host.run;
    if (!r || !guarded(r)) return;
    // Only the User's prompt is `messageType: "user"` (not `startsReply`, which also counts a from-null session-message).
    if (event.messageType === "user") {
      clearTrip(host, r);
      return;
    }
    const now = (host.options.now ?? Date.now)();
    r.turnStarts = [...r.turnStarts.filter((t) => t > now - WINDOW_MS), now];
    if (!r.tripped && r.turnStarts.length > r.info.loopGuardTurns) trip(host, r, now);
  });
}

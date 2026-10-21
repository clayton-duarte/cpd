import { isObject } from "./cpd.ts";
import type { Host } from "./host.ts";
import type { CommandCtx } from "./pi.ts";
import { dialogOpen, NONE_WAITING } from "./review-chat.ts";
import { showDialog } from "./review.ts";

export const PLAN_REVIEW_COMMAND = "plan-review";

const registered = new WeakSet<Host>();

/** §15.5 "/plan-review": reopens the dialog of this session's pending chat review (the Plan's latest, still pending in cpd); no new summary. pi has no way to remove a command, so it is registered once, when the session is a Plan Lead's. */
export function registerPlanReviewCommand(host: Host): void {
  if (registered.has(host)) return;
  registered.add(host);
  host.pi.registerCommand(PLAN_REVIEW_COMMAND, {
    description: "cpd: reopen the dialog of the Plan review that is waiting in this chat",
    async handler(_args: string, ctx: CommandCtx) {
      const r = host.run;
      if (!r || !r.info.planLead) return;
      let shown: unknown;
      try {
        shown = (await host.call(["plan", "show", "--plan", r.plan])).review;
      } catch (e) {
        host.deliver("plan show", e);
        return;
      }
      const entry = isObject(shown) && typeof shown.id === "string" ? r.reviews.get(shown.id) : undefined;
      if (!isObject(shown) || !entry || entry.surface !== "chat" || entry.status !== "pending" || shown.status !== "pending") {
        ctx.ui.notify(NONE_WAITING);
        return;
      }
      if (dialogOpen(r, entry.review)) return;
      showDialog(host, r, entry.review, typeof shown.title === "string" ? shown.title : undefined);
    },
  });
}

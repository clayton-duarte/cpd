import type { Host } from "./host.ts";
import type { CommandCtx, Ctx } from "./pi.ts";
import { refusal, RELOAD_COMMAND } from "./reload.ts";
import type { Run } from "./run.ts";

/** What §15.6 "Guards" looks at: pi idle, nothing pending, the state not blocked, the editor empty. */
export const guardOf = (r: Run, ctx: Ctx) => ({ idle: ctx.isIdle(), pending: ctx.hasPendingMessages(), state: r.facts.wanted().state, editorText: ctx.ui.getEditorText() });

/** The command the poll triggers (§15.6): reloads pi after an install. */
export function registerReloadCommand(host: Host): void {
  host.pi.registerCommand(RELOAD_COMMAND, {
    description: "cpd: reload pi after an install (runs by itself; does nothing unless pi is idle, unblocked and the editor is empty)",
    async handler(_args: string, ctx: CommandCtx) {
      const r = host.run;
      if (!r) return;
      const why = refusal(guardOf(r, ctx));
      if (why !== undefined) {
        r.log(`command: refused: ${why}`);
        return r.reload?.retry();
      }
      r.log("command: reloading");
      try {
        await ctx.reload(); // `ctx` is stale from here on
      } finally {
        r.log("command: ctx.reload() returned"); // a reload that happened is followed by the new instance's "start" line
        r.reload?.retry(); // still running: the reload did not happen, the next tick may try again
      }
    },
  });
}

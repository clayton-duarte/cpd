import { toCpdError } from "./cpd.ts";
import { registerAgentState } from "./agent-state.ts";
import { createHost, type Options } from "./host.ts";
import type { PiApi } from "./pi.ts";
import { RENDERED_TYPES } from "./render.ts";
import { registerReloadCommand } from "./reload-command.ts";
import { start, stop } from "./startup.ts";
import { registerReminderCheck } from "./turns.ts";

export type { Options };
export { QUESTION_LABEL } from "./review.ts";

/** commands.md §15: wires the concerns to pi. Start-up and shutdown: `startup.ts`; the follower's Events: `handle.ts`; the Plan review: `review.ts`; the agent state: `agent-state.ts`; Context updates and the reminder: `turns.ts`; the reload command: `reload-command.ts`. */
export function adapter(pi: PiApi, options: Options = {}): void {
  const host = createHost(pi, options);

  // commands.md §15.9: one renderer for every cpd customType; its style is read at `session_start` (startup.ts).
  for (const type of RENDERED_TYPES) pi.registerMessageRenderer(type, host.rendering.render);
  registerReloadCommand(host);
  registerReminderCheck(host);

  pi.on("session_start", async (event, ctx) => {
    try {
      await start(host, ctx, typeof event?.reason === "string" ? event.reason : "unknown");
    } catch (e) {
      host.deliver("start-up", e);
    }
  });

  pi.on("session_shutdown", async () => {
    try {
      await stop(host);
    } catch (e) {
      host.deliver("shutdown", e);
    }
  });

  registerAgentState(host);
}

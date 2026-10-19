import { Text } from "@earendil-works/pi-tui";
import { toCpdError } from "./cpd.ts";
import { registerAgentState } from "./agent-state.ts";
import { createHost, type Options } from "./host.ts";
import type { MessageRenderer, PiApi } from "./pi.ts";
import { registerReloadCommand } from "./reload-command.ts";
import { start, stop } from "./startup.ts";
import { registerReminderCheck } from "./turns.ts";
import { CONTEXT_TYPE, countsOf, summaryLine } from "./updates.ts";

export type { Options };
export { QUESTION_LABEL } from "./review.ts";

/** commands.md §15.7 "Display": collapsed, one dim line of counts; expanded, the full text. The model gets `content` either way. A message without counts (an older adapter's) is shown in full. */
export const renderContext: MessageRenderer = (message, { expanded, outputPad }, theme) => {
  const counts = countsOf(message.details);
  const text = expanded || counts === undefined ? String(message.content) : theme.fg("dim", summaryLine(counts));
  return new Text(text, outputPad, 0, (t) => theme.bg("customMessageBg", t));
};

/** commands.md §15: wires the concerns to pi. Start-up and shutdown: `startup.ts`; the follower's Events: `handle.ts`; the Plan review: `review.ts`; the agent state: `agent-state.ts`; Context updates and the reminder: `turns.ts`; the reload command: `reload-command.ts`. */
export function adapter(pi: PiApi, options: Options = {}): void {
  const host = createHost(pi, options);

  pi.registerMessageRenderer(CONTEXT_TYPE, renderContext);
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

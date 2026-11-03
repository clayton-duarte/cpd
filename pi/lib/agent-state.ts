import { callCpd, CpdError, isObject } from "./cpd.ts";
import type { Host } from "./host.ts";
import { boldPhi } from "./phi.ts";
import { switchModel } from "./retier.ts";
import { POLICY_SECTION } from "./policy.ts";
import { TIME_ZONE_SECTION, zoneLine } from "./time.ts";
import { isTyped, startsReply } from "./reply.ts";
import { type Facts, Reporter, stateArgs, type Wanted } from "./state.ts";

export const newReporter = (host: Host, id: string, facts: Facts) =>
  new Reporter(
    // No signal: the `stopped` of `session_shutdown` is sent after the abort, and `stop()` bounds its wait.
    async (w: Wanted) => {
      if (w.state === "idle" && w.reply) facts.reply = false; // sent once, with this call
      await callCpd(host.exec, stateArgs(w, id), { timeoutMs: host.timing.callTimeoutMs });
    },
    (e) => host.deliver("session state", e),
  );

/** §15.4: pi's hooks that change the facts the state is made of. `pi.on` has no unsubscribe, so each checks the run. */
export function registerAgentState(host: Host): void {
  const { pi } = host;

  // Who started the run. Returns nothing: it adds no message.
  pi.on("before_agent_start", function markReply(event) {
    const r = host.run;
    if (r) r.facts.reply = startsReply(event);
  });

  // §15.10: the policy section of the run, for every role. Returns nothing: it only sets the section.
  pi.on("before_agent_start", function policyHandler(event) {
    const r = host.run;
    const text = r?.policy;
    if (r === undefined || text === undefined) return;
    const sections = event?.systemPromptOptions?.sections;
    if (isObject(sections)) {
      sections[POLICY_SECTION] = text;
    } else if (!r.sectionsReported) {
      r.sectionsReported = true;
      host.deliver("policy section", new CpdError("sections_missing", "this pi gives no systemPromptOptions.sections: the policy is not delivered"));
    }
  });

  // §15.10: the time-zone line, every role, with or without a policy, built each turn. `policyHandler` reports a missing `sections`.
  pi.on("before_agent_start", function timeZoneHandler(event) {
    const sections = event?.systemPromptOptions?.sections;
    if (isObject(sections)) sections[TIME_ZONE_SECTION] = zoneLine(new Date());
  });

  pi.on("input", (event) => {
    const r = host.run;
    if (r && r.facts.active && isTyped(event)) r.facts.reply = true; // typing during a run (steer or follow-up) makes it a reply run
  });

  pi.on("agent_start", () => {
    const r = host.run;
    if (!r) return;
    r.facts.active = true;
    host.refresh(r);
  });

  // pi's dialogs (`ctx.ui.select/confirm/input/editor/custom`). A Worker never talks to the human (commands.md §15.4):
  // its dialog opens no reason; it asks its Lead with `cpd ask`.
  const onPrompt = (active: boolean) => {
    const r = host.run;
    if (!r || r.info.role === "worker") return;
    if (r.facts.prompt(active)) host.refresh(r);
  };
  pi.on("ui_prompt_start", () => onPrompt(true));
  pi.on("ui_prompt_end", () => onPrompt(false));

  pi.on("tool_result", (event) => {
    if (event.toolName === "ask_user_question") host.run?.facts.release();
  });

  pi.on("agent_settled", (_event, ctx) => {
    const r = host.run;
    if (!r) return;
    if (ctx.isIdle()) r.facts.active = false;
    host.refresh(r);
    if (r.retierDue) void switchModel(host, r); // §15.3 `session-retiered`: the Session's next idle
    r.reload?.nudge(); // a busy Lead's idle gaps can be shorter than the poll interval (§15.6 "Retries")
  });

  pi.on("session_info_changed", () => {
    const r = host.run;
    if (!r) return;
    const name = pi.getSessionName();
    if (name === undefined || name === "" || name === r.knownName) return;
    const key = r.info.key;
    if (key !== null) {
      if (name === key) return;
      r.knownName = key;
      pi.setSessionName(key);
      host.ctx?.ui.notify(boldPhi(`φ: ${key} is this session's key: its name cannot change.`));
      return;
    }
    r.knownName = name;
    void host.call(["session", "rename", name, "--session", r.id]).catch((e) => host.deliver("session rename", e));
  });
}

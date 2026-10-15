// cpd cockpit : publish this pi session's agent state into its tmux pane, as herdr's
// herdr-agent-state integration does for herdr. Same model: blocked while any `herdr:blocked` signal is open
// (cpd's adapter sends them for Worker questions), working between agent_start and agent_settled, else idle.
// It also publishes the session's identity (@session_id, @session_file, @session_name) at session_start and when
// pi renames the session (session_info_changed). The cockpit's views read these options and #{@agent_state},
// #{@agent_label}. Inactive outside the cockpit.
// It also publishes when the session needs the User, for the Context view's ●: @asking is 1 while a tool whose name
// starts with `ask_` runs, and after a `cpd_plan_review` or `plannotator` call until the next run starts (agent_start:
// the User's answer arrives as a custom message cpd sends with triggerTurn, which fires no `input`) or the User types.
// @settled_at is the epoch second of the last agent_settled (the view compares it with the @seen_at it stamps itself).
import { execFile } from "node:child_process";

export default function (pi: any) {
  const pane = process.env.TMUX_PANE;
  const socket = process.env.CPD_TUI_SOCKET;
  if (!pane || !socket) return;
  let active = false;
  let blocked = 0;
  let label = "";
  let last = "";
  const askTools = new Set<string>(); // toolCallIds of `ask_*` tools that are executing
  let reviewOpen = false; // a plan review was handed to the User and no run has started since
  let asking = "";
  const set = (state: string, text: string) =>
    execFile("tmux", ["-L", socket, "set", "-p", "-t", pane, "@agent_state", state, ";", "set", "-p", "-t", pane, "@agent_label", text], () => {});
  const tmux = (...args: string[]) => execFile("tmux", ["-L", socket, ...args], () => {});
  const publishIdentity = (ctx: any) => {
    const sm = ctx?.sessionManager;
    if (!sm) return;
    const opt = (name: string, value: string | undefined) => ["set", "-p", "-t", pane, name, value ?? ""];
    tmux(...opt("@session_id", sm.getSessionId()), ";", ...opt("@session_file", sm.getSessionFile()), ";", ...opt("@session_name", sm.getSessionName()));
  };
  const publish = () => {
    const state = blocked > 0 ? "blocked" : active ? "working" : "idle";
    const text = blocked > 0 ? label : "";
    if (`${state}|${text}` === last) return;
    last = `${state}|${text}`;
    set(state, text);
  };
  const publishAsking = () => {
    const value = askTools.size > 0 || reviewOpen ? "1" : "0";
    if (value === asking) return;
    asking = value;
    tmux("set", "-p", "-t", pane, "@asking", value);
  };
  pi.events.on("herdr:blocked", (d: any) => {
    if (d?.active) {
      blocked += 1;
      label = String(d.label ?? "");
    } else {
      blocked = Math.max(0, blocked - 1);
      if (blocked === 0) label = "";
    }
    publish();
  });
  pi.on("session_start", (_e: any, ctx: any) => {
    if (ctx?.mode !== "tui") return;
    active = ctx?.isIdle?.() === false;
    askTools.clear();
    reviewOpen = false;
    asking = "";
    publishAsking();
    publishIdentity(ctx);
    publish();
  });
  pi.on("session_info_changed", (_e: any, ctx: any) => publishIdentity(ctx));
  pi.on("agent_start", () => {
    active = true;
    reviewOpen = false; // a new run means the answer arrived: the decision message (triggerTurn) or the User's input
    publishAsking();
    publish();
  });
  pi.on("agent_settled", (_e: any, ctx: any) => {
    if (ctx?.isIdle?.() !== true) return;
    active = false;
    tmux("set", "-p", "-t", pane, "@settled_at", String(Math.floor(Date.now() / 1000)));
    publish();
  });
  pi.on("tool_execution_start", (e: any) => {
    if (String(e?.toolName ?? "").startsWith("ask_")) {
      askTools.add(String(e.toolCallId));
      publishAsking();
    }
  });
  pi.on("tool_execution_end", (e: any) => {
    const name = String(e?.toolName ?? "");
    if (name.startsWith("ask_")) askTools.delete(String(e.toolCallId));
    if (name === "cpd_plan_review" || name === "plannotator") reviewOpen = true;
    publishAsking();
  });
  pi.on("input", (e: any) => {
    if (e?.source !== "extension") {
      reviewOpen = false; // the User answered (a message injected by an extension is not an answer)
      publishAsking();
    }
    return { action: "continue" };
  });
  pi.on("session_shutdown", () => {
    askTools.clear();
    reviewOpen = false;
    publishAsking();
    set("stopped", "");
  });
}

// cpd cockpit : publish this pi session's agent state into its tmux pane, as herdr's
// herdr-agent-state integration does for herdr. Same model: blocked while any `herdr:blocked` signal is open
// (cpd's adapter sends them for Worker questions), working between agent_start and agent_settled, else idle.
// The cockpit's session tree reads #{@agent_state} and #{@agent_label}. Inactive outside the cockpit.
import { execFile } from "node:child_process";

export default function (pi: any) {
  const pane = process.env.TMUX_PANE;
  const socket = process.env.CPD_TUI_SOCKET;
  if (!pane || !socket) return;
  let active = false;
  let blocked = 0;
  let label = "";
  let last = "";
  const set = (state: string, text: string) =>
    execFile("tmux", ["-L", socket, "set", "-p", "-t", pane, "@agent_state", state, ";", "set", "-p", "-t", pane, "@agent_label", text], () => {});
  const publish = () => {
    const state = blocked > 0 ? "blocked" : active ? "working" : "idle";
    const text = blocked > 0 ? label : "";
    if (`${state}|${text}` === last) return;
    last = `${state}|${text}`;
    set(state, text);
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
    publish();
  });
  pi.on("agent_start", () => {
    active = true;
    publish();
  });
  pi.on("agent_settled", (_e: any, ctx: any) => {
    if (ctx?.isIdle?.() !== true) return;
    active = false;
    publish();
  });
  pi.on("session_shutdown", () => set("stopped", ""));
}

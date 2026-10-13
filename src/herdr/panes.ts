import { z } from "zod";
import { failure, usage } from "../lib/errors.ts";
import { assertNoControlChars, herdr } from "./herdr.ts";

/** `agent_session` as herdr reports it: for pi, `kind: "path"` and `value` = the session JSONL path (may not exist yet, t1 F10). */
export const AgentSession = z.looseObject({ kind: z.string(), value: z.string() });

export const Pane = z.looseObject({
  pane_id: z.string(),
  workspace_id: z.string(),
  agent_session: AgentSession.optional(),
});
export type Pane = z.infer<typeof Pane>;

const PaneInfo = z.looseObject({ pane: Pane });
const PaneList = z.looseObject({ panes: z.array(Pane) });
const Ok = z.looseObject({ type: z.string() });

export type SplitOptions = {
  direction: "right" | "down";
  cwd: string;
  env?: Record<string, string>;
  /** Always true: a split must never steal focus from the User. */
  noFocus: true;
};

/** `herdr pane split`; returns the new pane id (opaque, e.g. `w1:p2`). */
export async function splitPane(target: string, opts: SplitOptions): Promise<string> {
  assertNoControlChars("pane split target", target);
  const env = Object.entries(opts.env ?? {}).flatMap(([k, v]) => {
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(k)) throw usage("invalid_env_name", `invalid environment variable name: ${JSON.stringify(k)}`, { name: k });
    assertNoControlChars(`env ${k}`, v);
    return ["--env", `${k}=${v}`];
  });
  const r = await herdr(["pane", "split", target, "--direction", opts.direction, "--cwd", opts.cwd, ...env, "--no-focus"], PaneInfo);
  return r.pane.pane_id;
}

export async function closePane(id: string): Promise<void> {
  await herdr(["pane", "close", id], Ok);
}

export async function paneGet(id: string): Promise<Pane> {
  return (await herdr(["pane", "get", id], PaneInfo)).pane;
}

/** The workspace's root pane: the first pane `herdr pane list --workspace` reports. */
export async function rootPaneOfWorkspace(workspaceId: string): Promise<string> {
  const { panes } = await herdr(["pane", "list", "--workspace", workspaceId], PaneList);
  const root = panes[0];
  if (!root) throw failure("herdr_workspace_empty", `workspace ${workspaceId} has no panes`, { workspaceId });
  return root.pane_id;
}

/**
 * `herdr workspace close`. Closing a primary (non-linked) workspace while linked ones are open fails with
 * `herdr_workspace_group_close_required` (t3 F9); that is surfaced, never worked around, because closing the
 * children would close other Attempts' workspaces.
 */
export async function closeWorkspace(id: string): Promise<void> {
  await herdr(["workspace", "close", id], Ok);
}

/** Decision pro-d451: the tool `cpd_external_approve` (model: hostapprove.ts). The parsing and the dialog text are shared with the durable host. */
import { CpdError } from "./cpd.ts";
import { grantSlackSend, type SlackTool } from "./externalguard.ts";
import type { Host } from "./host.ts";

const registered = new WeakSet<Host>();

export const KINDS = ["pr_reply", "pr_post", "slack_send"] as const;
export type Parsed =
  | { kind: "pr_reply"; workflow: string; to: string; message: string | undefined; reason: string }
  | { kind: "pr_post"; workflow: string; reason: string }
  | { kind: "slack_send"; tool: SlackTool; channel: string; message: string; thread_ts: string | undefined; reason: string };

const text = (v: unknown): string | undefined => (typeof v === "string" && v !== "" ? v : undefined);

/** Validates the params at the boundary: a Parsed value, or a refusal `{code, message}` (before any dialog). */
export function parseExternal(params: unknown, role: string | undefined, planLead: boolean): Parsed | { code: string; message: string } {
  const p = (params ?? {}) as Record<string, unknown>;
  const reason = text(p.reason) ?? "";
  const kind = p.kind;
  if (kind !== "pr_reply" && kind !== "pr_post" && kind !== "slack_send") return { code: "bad_args", message: `kind must be one of ${KINDS.join(", ")}` };
  const lead = role === "lead" && planLead;
  if (kind === "slack_send") {
    if (role !== "lead" && role !== "manager") return { code: "not_a_lead", message: "the tool cpd_external_approve is for Lead and Manager Sessions only" };
    const channel = text(p.channel);
    const message = text(p.message);
    if (channel === undefined) return { code: "bad_args", message: "channel is required" };
    if (message === undefined) return { code: "bad_args", message: "message is required" };
    const tool = p.slack_tool ?? "slack_send_message";
    if (tool !== "slack_send_message" && tool !== "slack_send_message_draft") return { code: "bad_args", message: "slack_tool must be slack_send_message or slack_send_message_draft" };
    return { kind, tool, channel, message, thread_ts: text(p.thread_ts), reason };
  }
  if (!lead) return { code: "not_a_lead", message: "the tool cpd_external_approve (pr_reply, pr_post) is for Plan Lead Sessions only" };
  const workflow = text(p.workflow);
  if (workflow === undefined) return { code: "bad_args", message: "workflow is required" };
  if (kind === "pr_post") return { kind, workflow, reason };
  const to = text(p.to);
  if (to === undefined) return { code: "bad_args", message: "to (the item id) is required for pr_reply" };
  return { kind, workflow, to, message: text(p.message), reason };
}

/** The dialog message: names the exact target, and the whole text for a reply or a Slack message. */
export function approvalTitle(name: string, a: Parsed): string {
  if (a.kind === "pr_post") return `${name} asks to post the PR summary of workflow ${a.workflow}. Reason: ${a.reason}. Allow once: approve one cpd pr post (or repost) of this workflow.`;
  if (a.kind === "pr_reply") return `${name} asks to reply to item ${a.to} of workflow ${a.workflow}. Reason: ${a.reason}. Text:\n${a.message ?? "(none given)"}\nAllow once: approve one cpd pr reply to this item.`;
  const thread = a.thread_ts === undefined ? "" : `, thread ${a.thread_ts}`;
  return `${name} asks to send a Slack message (${a.tool}) to channel ${a.channel}${thread}. Reason: ${a.reason}. Message:\n${a.message}\nAllow once: approve exactly this one send.`;
}

/** The `cpd pr approve` argv of a pr kind. */
export const approveArgv = (a: Parsed & { kind: "pr_reply" | "pr_post" }): string[] =>
  a.kind === "pr_reply" ? ["pr", "approve", "--kind", "reply", "--workflow", a.workflow, "--to", a.to] : ["pr", "approve", "--kind", "post", "--workflow", a.workflow];

/** Records the one-shot Slack grant of an approved slack_send. */
export const grantOf = (a: Parsed & { kind: "slack_send" }): void => grantSlackSend(a.tool, { channel: a.channel, message: a.message, ...(a.thread_ts === undefined ? {} : { thread_ts: a.thread_ts }) });

export const APPROVED_TEXT = "Approved once: do the action now.";

/** Registers the tool `cpd_external_approve` (once per host). */
export function registerExternalApproveTool(host: Host): void {
  if (registered.has(host)) return;
  registered.add(host);
  host.pi.registerTool({
    name: "cpd_external_approve",
    label: "cpd external approve",
    description:
      "Ask the User to approve one external write: a PR reply (pr_reply: workflow, to, message), a PR post (pr_post: workflow) or a Slack send (slack_send: channel, message, thread_ts?). pr kinds: Plan Lead only; slack_send: Lead and Manager. One dialog; on Allow once, do the one action now.",
    parameters: {
      type: "object",
      properties: {
        kind: { type: "string", enum: [...KINDS] },
        workflow: { type: "string", description: "pr kinds: the Workflow key" },
        to: { type: "string", description: "pr_reply: the item id" },
        channel: { type: "string", description: "slack_send: the channel id" },
        message: { type: "string", description: "pr_reply and slack_send: the whole text" },
        thread_ts: { type: "string" },
        slack_tool: { type: "string", enum: ["slack_send_message", "slack_send_message_draft"] },
        reason: { type: "string", maxLength: 200, description: "One line: why" },
      },
      required: ["kind", "reason"],
      additionalProperties: false,
    },
    exposure: "model-only",
    async execute(_id: string, params: unknown) {
      const r = host.run;
      const a = parseExternal(params, r?.info.role, r?.info.planLead === true);
      if ("code" in a) throw new CpdError(a.code, a.message);
      if (!r) throw new CpdError("not_a_lead", "no session");
      const ui = host.ctx?.ui;
      if (!ui) throw new CpdError("no_ui", "the cockpit chat is needed to ask the User; there is no dialog in this session");
      const choice = await r.facts.ownPrompt(() => ui.select(approvalTitle(r.info.name ?? "the session", a), ["Allow once", "Deny"]));
      if (choice !== "Allow once") return { content: [{ type: "text" as const, text: "The User denied it. Do not try again." }], details: {} };
      if (a.kind === "slack_send") grantOf(a);
      else await host.call(approveArgv(a));
      return { content: [{ type: "text" as const, text: APPROVED_TEXT }], details: {} };
    },
  });
}

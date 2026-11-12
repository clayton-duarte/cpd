import { approvalPrompt } from "./state.ts";
/** commands.md §15.18: the tool `cpd_host_approve` and the raw-`cpd host approve` guard (model: §15.17). */
import { CpdError } from "./cpd.ts";
import type { Host } from "./host.ts";
import { PREFIXES, simpleCommands } from "./shell.ts";

const registered = new WeakSet<Host>();

/** The refused command as the reason shows it, or undefined when it may run. */
export function hostApproveBypass(command: string): string | undefined {
  for (const line of simpleCommands(command)) {
    let i = 0;
    while (i < line.length && (PREFIXES.has(line[i]!) || /^[A-Za-z_][A-Za-z0-9_]*=/.test(line[i]!))) i++;
    const words = line.slice(i);
    if (words[0] === "cpd" && words[1] === "host" && words[2] === "approve") return "cpd host approve";
  }
  return undefined;
}

/** The rule's reason, one line. */
export function reasonText(matched: string): string {
  return `φ: approvals are the User's: ask with the tool cpd_host_approve (commands.md §15.18); refused: ${matched}`;
}

type Params = { reason?: unknown };
const text = (v: unknown): string | undefined => (typeof v === "string" && v !== "" ? v : undefined);

/** commands.md §15.18: registers the tool `cpd_host_approve` (once per host). */
export function registerHostApproveTool(host: Host): void {
  if (registered.has(host)) return;
  registered.add(host);
  host.pi.registerTool({
    name: "cpd_host_approve",
    label: "cpd host approve",
    description:
      "Ask the User to approve repairing the cpd checkout (Manager only). One dialog; on Allow once, run cpd host approve and then cpd host repair now.",
    parameters: {
      type: "object",
      properties: {
        reason: { type: "string", maxLength: 200, description: "One line: why the repair is needed" },
      },
      required: ["reason"],
      additionalProperties: false,
    },
    exposure: "model-only",
    async execute(_id: string, params: Params) {
      const r = host.run;
      if (!r || r.info.role !== "manager") {
        throw new CpdError("not_a_manager", "the tool cpd_host_approve is for Manager Sessions only");
      }
      const reason = text(params?.reason) ?? "";
      const ui = host.ctx?.ui;
      if (!ui) throw new CpdError("no_ui", "the cockpit chat is needed to ask the User; there is no dialog in this session");
      const title = `${r.info.name} asks to repair the cpd checkout. Reason: ${reason}. Allow once: approve the repair. It discards tracked changes of ~/projects/cpd and resets main to origin/main after saving the diff and the old HEAD.`;
      const choice = await approvalPrompt(host, r, title, () => ui.select(title, ["Allow once", "Deny"]));
      if (choice !== "Allow once") return { content: [{ type: "text" as const, text: "The User denied it. Do not try again." }], details: {} };
      await host.call(["host", "approve"]);
      return { content: [{ type: "text" as const, text: "Approved: run cpd host repair now." }], details: {} };
    },
  });
}

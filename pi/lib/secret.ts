import { abortableApproval, approvalPrompt, DISMISSED } from "./state.ts";
import { CpdError } from "./cpd.ts";
import type { Host } from "./host.ts";

const registered = new WeakSet<Host>();
const OPEN = ["cpd", "homeserver"];
const ACTIONS = ["get", "list", "create", "edit", "delete"];

type Params = { action?: unknown; vault?: unknown; item?: unknown; field?: unknown; reason?: unknown };
const text = (v: unknown): string | undefined => (typeof v === "string" && v !== "" ? v : undefined);

/** commands.md §15.17: registers the tool `cpd_secret_approve` (once per host). */
export function registerSecretTool(host: Host): void {
  if (registered.has(host)) return;
  registered.add(host);
  host.pi.registerTool({
    name: "cpd_secret_approve",
    label: "cpd secret approve",
    description:
      "Ask the User to approve one cpd secret command on a vault other than cpd and homeserver (Lead and Manager only). One dialog; on Allow once, run the cpd secret command now.",
    parameters: {
      type: "object",
      properties: {
        action: { type: "string", enum: ACTIONS },
        vault: { type: "string" },
        item: { type: "string", description: "Every action but list" },
        field: { type: "string", description: "Required for get, create and edit" },
        reason: { type: "string", maxLength: 200, description: "One line: why you need it" },
      },
      required: ["action", "vault", "reason"],
      additionalProperties: false,
    },
    exposure: "model-only",
    execute(_id: string, params: Params, signal?: AbortSignal) {
      return abortableApproval(signal, DISMISSED, async (answered) => {
        const r = host.run;
        if (!r || (r.info.role !== "lead" && r.info.role !== "manager")) {
          throw new CpdError("not_a_lead", "the tool cpd_secret_approve is for Lead and Manager Sessions only");
        }
        const action = text(params?.action);
        const vault = text(params?.vault);
        const item = text(params?.item);
        const field = text(params?.field);
        const reason = text(params?.reason) ?? "";
        if (action === undefined || !ACTIONS.includes(action)) throw new CpdError("bad_args", `action must be one of ${ACTIONS.join(", ")}`);
        if (vault === undefined) throw new CpdError("bad_args", "vault is required");
        if (OPEN.includes(vault)) throw new CpdError("bad_vault", "these vaults need no approval; use cpd secret get|list|create|edit|delete (commands.md §24)");
        const ui = host.ctx?.ui;
        if (!ui) throw new CpdError("no_ui", "the cockpit chat is needed to ask the User; there is no dialog in this session");
        await host.call(["secret", "check"]);
        const title = `${r.info.name} asks to ${action} in vault ${vault}: item ${item ?? ""}, field ${field ?? ""}. Reason: ${reason}`;
        const choice = await approvalPrompt(host, r, title, () => ui.select(title, ["Allow once", "Deny"], { signal }), signal);
        answered();
        if (choice !== "Allow once") return { content: [{ type: "text" as const, text: "The User denied it. Do not try again." }], details: {} };
        const argv = ["secret", "approve", "--action", action, "--vault", vault];
        if (item !== undefined) argv.push("--item", item);
        if (field !== undefined) argv.push("--field", field);
        await host.call(argv);
        return { content: [{ type: "text" as const, text: "Approved for one command in the next 10 minutes: run it now." }], details: {} };
      });
    },
  });
}

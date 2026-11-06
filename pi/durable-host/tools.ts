// Contract §6.15 `tools.ts`: the host's registrations of `ask_user_question`, `cpd_secret_approve`, `cpd_host_approve`
// and `cpd_plan_review`, each a `defineTool` whose execute follows the pi reference (secret.ts, hostapprove.ts,
// review.ts, pi/ask-user-question). The G2 and G4 ask guards and noblock are P1.7, not here.
import { defineExtension, defineTool, type Extension, type ToolExecutionResult } from "@earendil-works/pi-durable";
import { Type, type Static } from "typebox";
import { normalizeQuestionParams } from "../ask-user-question/tool/normalize-params.ts";
import { buildQuestionnaireResponse, buildToolResult } from "../ask-user-question/tool/response-envelope.ts";
import { QuestionParamsSchema, type QuestionParams } from "../ask-user-question/tool/types.ts";
import { validateQuestionnaire } from "../ask-user-question/tool/validate-questionnaire.ts";
import { approvalTitle, approveArgv, APPROVED_TEXT, grantOf, KINDS, parseExternal } from "../lib/externalapprove.ts";
import type * as held from "./held.ts";
import type { Dialogs } from "./dialogs.ts";

export type RegisterHostToolsWhere = {
  role: "lead" | "manager";
  plan: string | undefined;
  name: string;
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  held: held.HeldApi;
  dialogs: Dialogs;
};

const ACTIONS = ["get", "list", "create", "edit", "delete"] as const;
const OPEN = ["cpd", "homeserver"];
const text = (v: unknown): string | undefined => (typeof v === "string" && v !== "" ? v : undefined);

/** pi's model-facing shape (`content` + `details`, as the tests and pi's envelope produce) with the pi-durable `output` the model actually sees. */
const piResult = (message: string, details: Record<string, unknown> = {}): ToolExecutionResult =>
  ({ content: [{ type: "text", text: message }], details, output: [{ type: "text", text: message }] }) as unknown as ToolExecutionResult;

export function registerHostTools(where: RegisterHostToolsWhere): Extension {
  return defineExtension({
    name: "cpd-host-tools",
    tools: [
      // Model-only, replay-safe (Q8): an interrupted questionnaire is asked again after a restart with a new id.
      defineTool({
        name: "ask_user_question",
        replay: "safe",
        callers: ["model"],
        description: "Ask the user one or more structured questions (1 to 4 questions, 2 to 4 options each) during execution.",
        parameters: QuestionParamsSchema,
        async execute(params) {
          const raw = (params ?? {}) as { questions?: unknown };
          if (!Array.isArray(raw.questions) || raw.questions.length === 0) {
            const noQuestions = buildToolResult("Error: At least one question is required", { answers: [], cancelled: true, error: "no_questions" });
            return piResult(noQuestions.content[0]!.text, { answers: [], cancelled: true, error: "no_questions" });
          }
          const typed = normalizeQuestionParams(params as unknown as QuestionParams);
          const validation = validateQuestionnaire(typed);
          if (!validation.ok) {
            return piResult(validation.message, { answers: [], cancelled: true, error: validation.error });
          }
          const result = await where.dialogs.questionnaire(typed.questions, { purpose: "ask_user_question" });
          // The envelope echoes the questions of the tool call (the answer's question field is the client's rendering of them).
          const aligned = { ...result, answers: result.answers.map((a) => ({ ...a, question: typed.questions[a.questionIndex]?.question ?? a.question })) };
          const envelope = buildQuestionnaireResponse(aligned, typed);
          return piResult(envelope.content[0]!.text, { cancelled: aligned.cancelled });
        },
      }),
      // Q8 C: an approval re-asked after a restart the User did not see could be answered by a client they did not mean to consent to.
      defineTool({
        name: "cpd_secret_approve",
        replay: "unsafe",
        callers: ["model"],
        description: "Ask the User to approve one cpd secret command on a vault other than cpd and homeserver (Lead and Manager only). One dialog; on Allow once, run the cpd secret command now.",
        parameters: Type.Object(
          {
            action: Type.Union(ACTIONS.map((a) => Type.Literal(a))),
            vault: Type.String(),
            item: Type.Optional(Type.String()),
            field: Type.Optional(Type.String()),
            reason: Type.String({ maxLength: 200 }),
          },
          { additionalProperties: false },
        ),
        async execute(params) {
          const p = params as { action?: unknown; vault?: unknown; item?: unknown; field?: unknown; reason?: unknown };
          const { action, vault, item, field, reason } = p;
          if (where.role !== "lead" && where.role !== "manager") {
            return piResult("the tool cpd_secret_approve is for Lead and Manager Sessions only");
          }
          const a = text(action);
          const v = text(vault);
          const i = text(item);
          const f = text(field);
          const why = text(reason) ?? "";
          if (a === undefined || !(ACTIONS as readonly string[]).includes(a)) return piResult(`action must be one of ${ACTIONS.join(", ")}`);
          if (v === undefined) return piResult("vault is required");
          if (OPEN.includes(v)) return piResult("these vaults need no approval; use cpd secret get|list|create|edit|delete (commands.md §24)");
          const choice = await where.dialogs.select(`${where.name} asks to ${a} in vault ${v}: item ${i ?? ""}, field ${f ?? ""}. Reason: ${why}`, ["Allow once", "Deny"], { purpose: "secret_approve" });
          if (choice !== "Allow once") return piResult("The User denied it. Do not try again.");
          const argv = ["secret", "approve", "--action", a, "--vault", v];
          if (i !== undefined) argv.push("--item", i);
          if (f !== undefined) argv.push("--field", f);
          await where.cpd(argv);
          return piResult("Approved for one command in the next 10 minutes: run it now.");
        },
      }),
      defineTool({
        name: "cpd_host_approve",
        replay: "unsafe",
        callers: ["model"],
        description: "Ask the User to approve repairing the cpd checkout (Manager only). One dialog; on Allow once, run cpd host approve and then cpd host repair now.",
        parameters: Type.Object({ reason: Type.String({ maxLength: 200 }) }, { additionalProperties: false }),
        async execute(params) {
          if (where.role !== "manager") {
            return piResult("the tool cpd_host_approve is for Manager Sessions only");
          }
          const reason = text((params as { reason?: unknown }).reason) ?? "";
          const choice = await where.dialogs.select(`${where.name} asks to repair the cpd checkout. Reason: ${reason}. Allow once: approve the repair. It discards tracked changes of ~/projects/cpd and resets main to origin/main after saving the diff and the old HEAD.`, ["Allow once", "Deny"], { purpose: "host_approve" });
          if (choice !== "Allow once") return piResult("The User denied it. Do not try again.");
          await where.cpd(["host", "approve"]);
          return piResult("Approved: run cpd host repair now.");
        },
      }),
      defineTool({
        name: "cpd_external_approve",
        replay: "unsafe",
        callers: ["model"],
        description:
          "Ask the User to approve one external write: a PR reply (pr_reply: workflow, to, message), a PR post (pr_post: workflow) or a Slack send (slack_send: channel, message, thread_ts?). pr kinds: Plan Lead only; slack_send: Lead and Manager. One dialog; on Allow once, do the one action now.",
        parameters: Type.Object(
          {
            kind: Type.Union(KINDS.map((k) => Type.Literal(k))),
            workflow: Type.Optional(Type.String()),
            to: Type.Optional(Type.String()),
            channel: Type.Optional(Type.String()),
            message: Type.Optional(Type.String()),
            thread_ts: Type.Optional(Type.String()),
            slack_tool: Type.Optional(Type.Union([Type.Literal("slack_send_message"), Type.Literal("slack_send_message_draft")])),
            reason: Type.String({ maxLength: 200 }),
          },
          { additionalProperties: false },
        ),
        async execute(params) {
          const a = parseExternal(params, where.role, where.role === "lead" && where.plan !== undefined);
          if ("code" in a) return piResult(`${a.code}: ${a.message}`);
          const choice = await where.dialogs.select(approvalTitle(where.name, a), ["Allow once", "Deny"], { purpose: "external_approve" });
          if (choice !== "Allow once") return piResult("The User denied it. Do not try again.");
          if (a.kind === "slack_send") grantOf(a);
          else await where.cpd(approveArgv(a));
          return piResult(APPROVED_TEXT);
        },
      }),
      // Replay-safe: the `plan-review` Event reopens the dialog after a restart (the pending entry survives); the tool itself opens no dialog.
      defineTool({
        name: "cpd_plan_review",
        replay: "safe",
        callers: ["model"],
        description: "Ask the User to review and approve the current cpd Plan (its draft Jobs): a short summary in the chat and one dialog (Approve, Change). Returns at once; the decision arrives later as a cpd message. Call it once per revision.",
        parameters: Type.Object({ title: Type.String({ maxLength: 72 }) }, { additionalProperties: false }),
        async execute(params) {
          if (where.role !== "lead" || where.plan === undefined) {
            return piResult("cpd_plan_review is only available in a Plan Lead's session");
          }
          const title = text((params as { title?: unknown }).title) ?? "";
          const out = await where.cpd(["plan", "review", "--plan", where.plan, "--title", title]);
          const review = (out as { review?: unknown }).review;
          const id = typeof review === "object" && review !== null && typeof (review as { id?: unknown }).id === "string" ? (review as { id: string }).id : undefined;
          const next = typeof out.next === "string" ? out.next : "The User reviews the plan summary in the chat. Stop and wait for the decision message.";
          return piResult(next, { review: id, already: out.already === true });
        },
      }),
    ],
  });
}

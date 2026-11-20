import type { Dialog, Question, TranscriptItem } from "../../../protocol/domain.ts";
import type { HostFrame } from "../client.ts";
import { arr, obj, str } from "./validation.ts";

function questionsOf(raw: unknown): Question[] {
  return arr(raw, "questions").map((q) => {
    const o = obj(q, "question");
    const options = arr(o.options, "question options").map((opt) => {
      const x = obj(opt, "question option");
      return { label: str(x.label, "option label"), description: str(x.description ?? "", "option description"), ...(typeof x.preview === "string" ? { preview: x.preview } : {}) };
    });
    return { header: str(o.header, "question header"), question: str(o.question, "question text"), options, ...(o.multiSelect === true ? { multiSelect: true } : {}) };
  });
}

export function dialogFromRequest(
  req: Extract<HostFrame, { type: "ui_request" }>,
  transcript: readonly TranscriptItem[],
): Dialog | { kind: "editor"; id: string } | { kind: "unsupported"; id: string; method: string; purpose: string } {
  if (req.method === "questionnaire") {
    return { id: req.id, method: "ask_user_question", questions: questionsOf(req.questions), interactive: true };
  }
  if (req.method === "select" && req.purpose === "plan_review" && JSON.stringify(req.options) === '["Approve","Change"]') {
    let title = "Plan review";
    let summary = "";
    for (const item of [...transcript].reverse()) {
      if (item.kind === "tool" && item.name === "cpd_plan_review" && typeof item.args.title === "string") {
        title = item.args.title;
        break;
      }
    }
    for (const item of [...transcript].reverse()) {
      if (item.kind === "fold" && item.foldKind === "cpd-plan-summary") {
        summary = item.body;
        break;
      }
    }
    return { id: req.id, method: "cpd_plan_review", title, summary, interactive: true };
  }
  if (req.method === "select" && (req.purpose === "secret_approve" || req.purpose === "host_approve" || req.purpose === "external_approve")) {
    const title = typeof req.title === "string" ? req.title : "Approval";
    return {
      id: req.id,
      method: "ask_user_question",
      questions: [
        {
          header: "Approval",
          question: title,
          options: [
            { label: "Allow once", description: "" },
            { label: "Deny", description: "" },
          ],
        },
      ],
      interactive: true,
    };
  }
  if (req.method === "editor") return { kind: "editor", id: req.id };
  return { kind: "unsupported", id: req.id, method: req.method, purpose: req.purpose };
}

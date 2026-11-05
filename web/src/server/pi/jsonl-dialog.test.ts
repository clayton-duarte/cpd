import { expect, it } from "vitest";
import { normalizeText } from "./jsonl.ts";

it("an open cpd_plan_review dialog is interactive", () => {
  const text = JSON.stringify({ type: "custom", id: "r7", customType: "cpd-plan-review", data: { review: "7", status: "pending" } });
  const { openDialogs } = normalizeText(`${text}\n`);
  expect(openDialogs).toHaveLength(1);
  expect(openDialogs[0]).toMatchObject({ id: "review:7", method: "cpd_plan_review", interactive: true });
});

it("an open ask_user_question call is an interactive dialog", () => {
  const call = JSON.stringify({
    type: "message",
    id: "a1",
    message: {
      role: "assistant",
      stopReason: "toolUse",
      content: [
        {
          type: "toolCall",
          id: "q1",
          name: "ask_user_question",
          arguments: { questions: [{ header: "H", question: "Q?", options: [{ label: "A", description: "a" }, { label: "B", description: "b" }] }] },
        },
      ],
    },
  });
  const { openDialogs } = normalizeText(`${call}\n`);
  expect(openDialogs[0]).toMatchObject({ id: "call:q1", method: "ask_user_question", interactive: true });
});

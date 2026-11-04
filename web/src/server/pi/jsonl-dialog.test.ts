import { expect, it } from "vitest";
import { normalizeText } from "./jsonl.ts";

it("an open cpd_plan_review dialog is interactive", () => {
  const text = JSON.stringify({ type: "custom", id: "r7", customType: "cpd-plan-review", data: { review: "7", status: "pending" } });
  const { openDialogs } = normalizeText(`${text}\n`);
  expect(openDialogs).toHaveLength(1);
  expect(openDialogs[0]).toMatchObject({ id: "review:7", method: "cpd_plan_review", interactive: true });
});

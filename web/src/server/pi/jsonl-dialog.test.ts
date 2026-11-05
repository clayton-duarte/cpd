import { expect, it } from "vitest";
import { createNormalizer, normalizeText } from "./jsonl.ts";

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

const entry = (id: string, review: string, status: string, extra: Record<string, unknown> = {}): string =>
  JSON.stringify({ type: "custom", id, customType: "cpd-plan-review", data: { review, status, ...extra } });
const summaryLine = (id: string, review: string, text: string): string =>
  JSON.stringify({ type: "custom_message", id, customType: "cpd-plan-summary", content: text, display: true, details: { plan: "p", review } });
const filler = (id: string): string => JSON.stringify({ type: "session_info", id, name: "n" });
const reviewDialogs = (text: string) => normalizeText(`${text}\n`).openDialogs.filter((d) => d.method === "cpd_plan_review");

it("a review entry before its summary opens the dialog with that summary", () => {
  const text = [entry("e1", "r1", "pending"), filler("x1"), summaryLine("s1", "r1", "SUMMARY-ONE")].join("\n");
  const d = reviewDialogs(text).find((x) => x.id === "review:r1");
  expect(d).toMatchObject({ summary: "SUMMARY-ONE" });
});

it("a later review never shows an earlier review's summary", () => {
  const first = [entry("e1", "r1", "pending"), filler("x1"), summaryLine("s1", "r1", "SUMMARY-ONE"), entry("e2", "r1", "approved")];
  const withSummary = reviewDialogs([...first, entry("e3", "r2", "pending"), filler("x2"), summaryLine("s2", "r2", "SUMMARY-TWO")].join("\n"));
  expect(withSummary.find((x) => x.id === "review:r2")).toMatchObject({ summary: "SUMMARY-TWO" });
  expect(JSON.stringify(withSummary)).not.toContain("SUMMARY-ONE");
  const without = reviewDialogs([...first, entry("e3", "r2", "pending")].join("\n"));
  expect(without.find((x) => x.id === "review:r2")).toMatchObject({ summary: "" });
  expect(JSON.stringify(without)).not.toContain("SUMMARY-ONE");
});

it("a review entry that carries summary and title opens the dialog with them", () => {
  const d = reviewDialogs(entry("e1", "r1", "pending", { summary: "CARRIED-SUMMARY", title: "Carried title" }));
  expect(d).toHaveLength(1);
  expect(d[0]).toMatchObject({ id: "review:r1", title: "Carried title", summary: "CARRIED-SUMMARY" });
});

it("the summary arriving after its review emits the dialog again with the summary", () => {
  const n = createNormalizer();
  n.push(entry("e1", "r1", "pending"), { line: 1, offset: 0 });
  const second = n.push(summaryLine("s1", "r1", "SUMMARY-ONE"), { line: 2, offset: 100 });
  expect(second.dialogsOpened.find((x) => x.id === "review:r1")).toMatchObject({ summary: "SUMMARY-ONE" });

  const m = createNormalizer();
  m.push(entry("e1", "r1", "pending"), { line: 1, offset: 0 });
  m.push(entry("e2", "r1", "approved"), { line: 2, offset: 100 });
  const late = m.push(summaryLine("s1", "r1", "SUMMARY-ONE"), { line: 3, offset: 200 });
  expect(late.dialogsOpened).toEqual([]);
});

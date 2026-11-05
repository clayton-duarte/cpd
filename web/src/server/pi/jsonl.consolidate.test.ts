import { describe, expect, it } from "vitest";
import type { Dialog } from "../../protocol/domain.ts";
import { createDialect, type DurableTranscript } from "../durable/frames.ts";

// CONTRACT 11.4 "durable plan review": `frames.ts` keeps the pending `cpd-plan-review` entries and the
// dialect exposes the latest one as `transcript.openDialog`. (The pi JSONL normaliser has the same
// contract through `openDialogs`; it is covered by jsonl-dialog.test.ts.)

type Obj = Record<string, unknown>;
type WithOpenDialog = DurableTranscript & { readonly openDialog?: Dialog };

const review = (id: number, review: string | number, status: string): Obj => ({
  id,
  conversationId: "root",
  kind: "cpd-plan-review",
  data: { review, status },
});
const assistantCall = (id: number, title: string): Obj => ({
  id,
  conversationId: "root",
  kind: "pi.assistant",
  model: [{ role: "assistant", content: [{ type: "toolCall", id: `c${id}`, name: "cpd_plan_review", arguments: { title } }] }],
});
const summary = (id: number, n: string, body: string): Obj => ({
  id,
  conversationId: "root",
  kind: "cpd-plan-summary",
  data: { review: n },
  model: [{ role: "user", content: body }],
});

function open(entries: Obj[]): WithOpenDialog {
  const dialect = createDialect(2);
  dialect.applyBase({
    view: { conversation: "root", entries, docs: { "pi.agent": {}, "pi.live": {}, "pi.inbox": { items: [] }, "pi.usage": { models: {}, tools: {} } } },
    epoch: "e1",
    seq: 1,
  });
  return dialect.transcript as WithOpenDialog;
}

describe("durable frames plan review entries", () => {
  it("jsonl parses cpd-plan-review entries into review items", () => {
    const t = open([review(1, "7", "pending")]);
    expect(t.openDialog).toMatchObject({ id: "review:7", method: "cpd_plan_review", interactive: true });
    expect(t.items).toEqual([]); // a review entry is state, never a transcript item
    expect(open([review(1, 8, "pending")]).openDialog).toMatchObject({ id: "review:8" }); // review may be a number
    expect(open([review(1, "7", "approved")]).openDialog).toBeUndefined(); // not pending: no dialog
  });

  it("jsonl exposes the latest pending review as the transcript open dialog", () => {
    const t = open([
      assistantCall(1, "Ship the thing"),
      summary(2, "8", "the summary body"),
      review(3, "7", "pending"),
      review(4, "8", "pending"),
    ]);
    expect(t.openDialog).toEqual({ id: "review:8", method: "cpd_plan_review", title: "Ship the thing", summary: "the summary body", interactive: true });
    const later = open([review(1, "7", "pending"), review(2, "8", "pending"), review(3, "8", "rejected")]);
    expect(later.openDialog).toMatchObject({ id: "review:7", title: "Plan review 7" });
  });

  it("jsonl takes fold data from message details when entry data is empty", () => {
    const t = open([
      { id: 1, conversationId: "root", kind: "cpd-context", data: {}, model: [{ role: "user", content: "first line\nmore", details: { summary: "from details" } }] },
      { id: 2, conversationId: "root", kind: "cpd-plan-summary", data: {}, model: [{ role: "user", content: "body", details: { review: "9" } }] },
    ]);
    const titles = t.items.flatMap((i) => (i.kind === "fold" ? [i.title] : []));
    expect(titles).toEqual(["from details", "Plan review 9 summary"]);
  });
});

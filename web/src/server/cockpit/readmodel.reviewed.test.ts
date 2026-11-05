import { expect, it } from "vitest";
import type { Cpd } from "./cli.ts";
import { createReadModel } from "./readmodel.ts";

type Obj = Record<string, unknown>;

const planShow = (reviewedPr: Obj | null): Obj => ({
  ok: true,
  plan: { id: "p1", context: "codereview", title: "Review acme/api#3", phase: "dispatch", archived_at: null, lead_session: null },
  counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
  jobs: [],
  workflows: [],
  review: null,
  escalations: [],
  reviewed_pr: reviewedPr,
  events_cursor: 20,
});

function cpdWith(show: Obj): Cpd {
  return {
    async json(args) {
      if (args.join(" ").startsWith("plan show")) return show;
      throw new Error(`unexpected ${args.join(" ")}`);
    },
  };
}

it("readmodel plan view carries the reviewed PR of a review plan", async () => {
  const pr = { repo: "acme/api", number: 3, url: "https://github.com/acme/api/pull/3", title: "Fix the thing", state: "open" };
  const withPr = await createReadModel(cpdWith(planShow(pr))).loadPanes(new Set(["plan"]), "p1");
  expect(withPr.plan!.reviewedPr).toEqual(pr);
  const unknown = await createReadModel(cpdWith(planShow({ ...pr, state: null }))).loadPanes(new Set(["plan"]), "p1");
  expect(unknown.plan!.reviewedPr).toEqual({ ...pr, state: null });
  const none = await createReadModel(cpdWith(planShow(null))).loadPanes(new Set(["plan"]), "p1");
  expect(none.plan!.reviewedPr).toBeNull();
});

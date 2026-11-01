import assert from "node:assert/strict";
import { test } from "node:test";
import { parsePending, pendingSection } from "../lib/updates.ts";

// Plan `explore-view` (commands.md §3.12, §15.7): the plan-level explore Jobs of `cpd context pending` are in
// `parsePending` and in the Manager's `Pending work:` line (`pendingSection`). The literals are plain JSON, never a
// typed `PendingJob`/`PendingPlan`, so this file compiles before the implementation.

/** A plan-level explore Job of `cpd context pending`, as plain JSON. */
const planJob = (key: string, kind: string, status: string, tier: string, title: string, touches: string[]) => ({ key, kind, status, tier, title, touches, waiting_on: [] });

test("parsePending: a Plan's plan-level jobs; a Plan without jobs is bad_output", () => {
  const out = {
    ok: true,
    context: "ctx",
    plans: [
      {
        id: "live",
        title: "Title of live",
        phase: "dispatch",
        lead_session: null,
        jobs: [planJob("look", "scout", "queued", "H2", "Title look", ["explore/look.md"]), planJob("dig", "research", "waiting", "S2", "Title dig", ["explore/dig.md"])],
        workflows: [
          {
            key: "api",
            repo: "/r/api",
            base: "main",
            status: "dispatch",
            lead_branch: "cpd/live/api",
            pull_request: null,
            jobs: [{ key: "w1", status: "queued", title: "Title w1", touches: ["src/w1.rs"] }],
          },
        ],
      },
    ],
  };
  const parsed = parsePending(out) as unknown as { jobs: unknown[]; workflows: { key: string; jobs: unknown[] }[] }[];
  // The plan-level `jobs`: key, kind, status, tier and title, in the JSON's order.
  assert.deepEqual(parsed[0].jobs, [
    { key: "look", kind: "scout", status: "queued", tier: "H2", title: "Title look" },
    { key: "dig", kind: "research", status: "waiting", tier: "S2", title: "Title dig" },
  ]);
  // And `workflows` is still read, with its own Jobs.
  assert.equal(parsed[0].workflows.length, 1);
  assert.equal(parsed[0].workflows[0].key, "api");
  assert.equal((parsed[0].workflows[0].jobs[0] as { key: string }).key, "w1");
  // A Plan without `jobs` is bad_output, like one without `workflows`.
  assert.throws(() => parsePending({ plans: [{ id: "p", phase: "dispatch", workflows: [] }] }), { code: "bad_output" });
  assert.throws(() => parsePending({ plans: [{ id: "p", phase: "dispatch", jobs: [] }] }), { code: "bad_output" });
});

test("pendingSection: the plan-jobs segment, its order, and a plan-only-Jobs Plan", () => {
  const plan = (id: string, jobs: any[], workflows: any[]) => ({ id, phase: "dispatch", jobs, workflows });
  const jobs = (...statuses: string[]) => statuses.map((status, i) => ({ key: `j${i + 1}`, kind: "scout", status, tier: "H2", title: `J${i + 1}` }));
  const pr = (key: string, status: string, number: number | null) => ({ key, status, number, jobs: [{ key: `${key}-w`, status: "queued", title: "W" }] });
  const onlyJobs = plan("solo", jobs("queued", "queued"), []);
  const withBoth = plan("live", jobs("running", "queued"), [pr("api", "dispatch", 7)]);
  const nothing = plan("empty", [], []);
  // The `jobs` segment comes first, then the `Workflow` segments; a Plan with neither is omitted.
  assert.deepEqual(pendingSection([withBoth, onlyJobs, nothing]), [
    "Pending work:",
    "- live (dispatch): jobs 1 running, 1 queued; Workflow api 1 queued",
    "- solo (dispatch): jobs 2 queued",
  ]);
});

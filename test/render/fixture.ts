import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { withLedger } from "../../src/ledger/bd.ts";
import { addJob, type JobSpec } from "../../src/ledger/jobs.ts";
import { cpdHome } from "../../src/lib/paths.ts";
import { renderPlan } from "../../src/render/plan.ts";
import { renderReviewBrief } from "../../src/render/review.ts";
import { renderWorkerSpec } from "../../src/render/spec.ts";
import { CATALOG, jobId, newPlan, sandbox } from "../ledger/helpers.ts";

export const WF = "render";
export const API_WORKTREE = "/work/trees/render--api-a2";

const CONFIG = {
  version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5,
  prPollSeconds: 120, herdrSession: null,
  tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
  leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
};

export function writeConfig(reviewMode: "fork" | "fresh"): void {
  mkdirSync(cpdHome(), { recursive: true });
  writeFileSync(join(cpdHome(), "config.json"), JSON.stringify({ ...CONFIG, reviewMode }));
}

type AttemptFixture = { number: number; tier?: string; worktree?: string; closed?: { outcome: string; cause?: string; feedback?: string[] } };

/** Test-only: an Attempt issue under a Job with the metadata the lifecycle would have recorded (ledger-attempts is not in this base). */
export async function makeAttempt(wf: string, key: string, a: AttemptFixture): Promise<void> {
  const parent = await jobId(wf, key);
  await withLedger(wf, "test attempt", async (l) => {
    const meta = { number: a.number, tier: a.tier, worktree: a.worktree, ...(a.closed ? { outcome: a.closed.outcome, ...(a.closed.cause ? { cause: a.closed.cause } : {}) } : {}) };
    const created: any = await l.write(["create", `--title=${key} attempt ${a.number}`, "-t", "attempt", "--parent", parent, "-s", "in_progress", "--metadata", JSON.stringify(meta)]);
    if (a.closed) {
      for (const f of a.closed.feedback ?? []) await l.write(["comments", "add", created.id, f]);
      await l.write(["close", created.id, "--reason", a.closed.outcome]);
    }
  });
}

const spec = (s: Partial<JobSpec> & Pick<JobSpec, "goal">): JobSpec => ({ context: "", touches: ["src/x.ts"], acceptance: ["npm test passes"], outOfScope: [], ...s });

/**
 * Three Jobs: `schema`; `api` (needs schema) on Attempt 2 after a rejected Attempt 1; `docs` (needs both,
 * with Markdown that needs sanitising). All drafts, so the plan is rendered in phase `plan`.
 */
export async function buildFixture(): Promise<void> {
  const { repo } = sandbox();
  writeConfig("fork");
  await newPlan(WF, repo);
  await addJob(WF, {
    key: "schema", title: "Define the schema", tier: "S3", reviewTier: "S4", needs: [],
    spec: spec({
      goal: "Add the zod schema for payments.",
      context: "The API job builds on this.\n\nSee `docs/payments.md`.",
      touches: ["src/schema.ts", "test/schema.test.ts"],
      acceptance: ["`npm test -- test/schema.test.ts` passes", "`src/schema.ts` exports `Payment`"],
      outOfScope: ["Any HTTP code"],
    }),
  }, CATALOG);
  await addJob(WF, {
    key: "api", title: "Payments API | v1", tier: "S3", reviewTier: "S4", needs: ["schema"],
    spec: spec({
      goal: "Expose `POST /payments` using the schema.",
      context: "Follow the existing handler style.",
      touches: ["src/api.ts", "test/api.test.ts"],
      acceptance: ["`npm test` passes", "`curl -s -X POST localhost:3000/payments -d '{}'` returns 400"],
      outOfScope: ["Auth", "Rate limiting"],
    }),
  }, CATALOG);
  await addJob(WF, {
    key: "docs", title: "Document the API", tier: "H3", reviewTier: "S3", needs: ["schema", "api"],
    spec: spec({
      goal: "Write the docs.\n\nUse <b>bold</b> sparingly and cite sources[^1].\n\n```html\n<div>kept as is</div>\n```",
      context: "Inline `<code>` stays too.",
      touches: ["docs/api.md"],
      acceptance: ["`docs/api.md` mentions every route"],
      outOfScope: [],
    }),
  }, CATALOG);
  await makeAttempt(WF, "schema", { number: 1, tier: "S3", worktree: "/work/trees/render--schema-a1" });
  await makeAttempt(WF, "api", {
    number: 1, tier: "S3", worktree: "/work/trees/render--api-a1",
    closed: { outcome: "rejected", cause: "review_rejected", feedback: ["REJECTED: `POST /payments` returns 500 on an empty body.\n\nAdd the 400 path and a test for it."] },
  });
  await makeAttempt(WF, "docs", { number: 1, tier: "H3", worktree: "/work/trees/render--docs-a1" });
  await makeAttempt(WF, "api", { number: 2, tier: "S4", worktree: API_WORKTREE });
}

export type Rendered = Record<string, string>;

const normalise = (s: string) => s.replaceAll(cpdHome(), "$CPD_HOME");

/** Every document the goldens pin, by golden file name. */
export async function renderAll(): Promise<Rendered> {
  const read = (p: string) => normalise(readFileSync(p, "utf8"));
  const out: Rendered = {};
  out["worker-schema-a1.md"] = read(await renderWorkerSpec(WF, "schema-a1"));
  out["worker-docs-a1.md"] = read(await renderWorkerSpec(WF, "docs-a1"));
  out["worker-api-a2.md"] = read(await renderWorkerSpec(WF, "api-a2"));
  const inputs = { diffPath: "/state/reviews/api-a2.diff", gatePath: "/state/reviews/api-a2.gate.txt", worktree: API_WORKTREE };
  out["review-api-a2.md"] = read(await renderReviewBrief(WF, "api-a2", inputs));
  writeConfig("fresh");
  out["review-api-a2-fresh.md"] = read(await renderReviewBrief(WF, "api-a2", inputs));
  writeConfig("fork");
  out["plan.md"] = read((await renderPlan(WF)).path);
  return out;
}

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { planFingerprint } from "../../src/ledger/plan.ts";
import { cpdHome, planPath, specPath } from "../../src/lib/paths.ts";
import { planMarkdown, renderPlan } from "../../src/render/plan.ts";
import { renderReviewBrief } from "../../src/render/review.ts";
import { renderWorkerSpec } from "../../src/render/spec.ts";
import { TemplateError } from "../../src/render/template.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { addDraft } from "../ledger/helpers.ts";
import { API_WORKTREE, buildFixture, makeAttempt, renderAll, WF, writeConfig, type Rendered } from "./fixture.ts";

const golden = (name: string) => readFileSync(fileURLToPath(new URL(`./golden/${name}`, import.meta.url)), "utf8");

let rendered: Rendered;

test("setup: a Ledger fixture with 3 Jobs, one on Attempt 2 with feedback", async () => {
  await buildFixture();
  rendered = await renderAll();
});

// Goldens are updated deliberately: `CPD_REGEN_GOLDENS=1 node test/render/regen.ts`, then review the diff. Tests never write them.
for (const name of ["worker-schema-a1.md", "worker-docs-a1.md", "worker-api-a2.md", "review-api-a2.md", "review-api-a2-fresh.md", "plan.md"]) {
  test(`golden: ${name}`, () => {
    assert.equal(rendered[name], golden(name));
  });
}

test("Worker Spec for Attempt 1 has the exact submit and ask commands, ids and --wf", () => {
  const s = rendered["worker-schema-a1.md"]!;
  assert.ok(s.includes("\n    cpd attempt submit schema-a1 --wf render\n"), "submit");
  assert.ok(s.includes('\n      cpd ask schema-a1 "<your question>" --wf render\n'), "ask");
  for (const code of ["dirty_tree", "no_commits", "gate_tampered", "check_failed"]) assert.ok(s.includes(`\`${code}\``), code);
  assert.ok(!s.includes("## Previous Attempts"), "no previous Attempts for Attempt 1");
  assert.ok(s.includes("Your Tier is `S3`"));
  assert.ok(!s.includes("`gate_failed`") && !/Run `\.\/verify\.sh`/.test(s), "the Worker no longer runs or hears about the Gate");
  assert.match(s, /`pre-commit` hook/);
  assert.match(s, /full Gate runs later, when the Lead opens the PR/);
});

test("Worker Spec for Attempt 2 shows the previous Attempt's outcome, cause and Reviewer feedback", () => {
  const s = rendered["worker-api-a2.md"]!;
  const prev = s.slice(s.indexOf("## Previous Attempts"), s.indexOf("## Finish"));
  assert.match(prev, /### Attempt 1 \(Tier `S3`\)/);
  assert.match(prev, /Outcome: rejected/);
  assert.match(prev, /Cause: review_rejected/);
  assert.match(prev, /> REJECTED: `POST \/payments` returns 500 on an empty body\./);
  assert.ok(s.includes("Your Tier is `S4`") && s.includes(`\`${API_WORKTREE}\``));
});

test("rendering is idempotent and writes where the contract says", async () => {
  const path = await renderWorkerSpec(WF, "api-a2");
  assert.equal(path, `${cpdHome()}/workflows/${WF}/specs/api-a2.md`);
  assert.equal(readFileSync(path, "utf8").replaceAll(cpdHome(), "$CPD_HOME"), rendered["worker-api-a2.md"]);
});

test("review brief: read-only Reviewer, four inputs, rubric, one verdict block; plan only in fresh mode", () => {
  const fork = rendered["review-api-a2.md"]!;
  const fresh = rendered["review-api-a2-fresh.md"]!;
  for (const s of [fork, fresh]) {
    assert.match(s, /read-only/);
    for (const p of ["specs/api-a2.md", "/state/reviews/api-a2.diff", "/state/reviews/api-a2.gate.txt", API_WORKTREE]) assert.ok(s.includes(p), p);
    assert.match(s, /every acceptance item/i);
    const blocks = s.match(/```json\n[\s\S]*?\n```/g) ?? [];
    assert.equal(blocks.length, 1);
    assert.ok(s.trimEnd().split("\n").slice(-4).join("\n").includes("`verdict`"), "verdict format notes close the brief");
    assert.deepEqual(Object.keys(JSON.parse(blocks[0]!.slice(8, -4))), ["verdict", "feedback"]);
  }
  assert.ok(!fork.includes("plan.md"), "fork: no plan pointer");
  assert.ok(fresh.includes("$CPD_HOME/workflows/render/plan.md"), "fresh: plan pointer");
});

test("fresh mode leaves the plan it points to on disk", async () => {
  writeConfig("fresh");
  try {
    await renderReviewBrief(WF, "api-a2", { diffPath: "/d", gatePath: "/g", worktree: "/w" });
  } finally {
    writeConfig("fork");
  }
  assert.ok(existsSync(planPath(WF)));
});

test("the plan: a GFM table, a Mermaid graph TD with one edge per Dependency, one section per Job", () => {
  const p = rendered["plan.md"]!;
  const lines = p.split("\n");
  const header = lines.findIndex((l) => l === "| Key | Title | Tier | Needs | Touches |");
  assert.ok(header >= 0 && lines[header + 1] === "|---|---|---|---|---|");
  assert.equal(lines.slice(header + 2).filter((l) => l.startsWith("| ")).length >= 3, true);

  const mermaid = /```mermaid\n([\s\S]*?)\n```/.exec(p);
  assert.ok(mermaid, "mermaid block");
  const m = mermaid![1]!.split("\n");
  assert.equal(m[0], "graph TD");
  assert.equal(m.filter((l) => l.includes("-->")).length, 3, "schema→api, schema→docs, api→docs");

  const sections = lines.filter((l) => /^## `[^`]+`: /.test(l));
  assert.deepEqual(sections, ["## `schema`: Define the schema", "## `api`: Payments API | v1", "## `docs`: Document the API"]);
});

test("the plan has no footnotes and no raw HTML, even when Job text has some", () => {
  const p = rendered["plan.md"]!;
  const prose = p
    .replace(/^(`{3,}|~{3,})[^\n]*\n[\s\S]*?\n\1[ \t]*$/gm, "")
    .replace(/`[^`\n]*`/g, "");
  assert.doesNotMatch(prose, /<\/?[A-Za-z!?][^>]*>/, "raw HTML");
  assert.doesNotMatch(prose, /(?<!\\)\[\^/, "footnote reference");
  assert.doesNotMatch(p, /^\[\^[^\]]+\]:/m, "footnote definition");
  assert.ok(p.includes("&lt;b>bold&lt;/b>"), "the tag in Job text is escaped");
  assert.ok(p.includes("<div>kept as is</div>"), "fenced code is left alone");
  assert.ok(p.includes("Payments API \\| v1"), "a pipe in a title does not break the table");
});

test("renderPlan returns planFingerprint and writes to --out; the plan of no Jobs is still valid", async () => {
  const r = await renderPlan(WF, `${cpdHome()}/elsewhere/plan.md`);
  assert.equal(r.fingerprint, await planFingerprint(WF));
  assert.equal(readFileSync(r.path, "utf8").replaceAll(cpdHome(), "$CPD_HOME"), rendered["plan.md"]);
  assert.equal(planMarkdown("T", "context", []), "# Plan: T\n\nPhase: `context`\n\nNo Jobs yet.\n");
});

test("a missing template value is a named error; a missing Attempt is not_found", async () => {
  await addDraft(WF, "bare");
  await makeAttempt(WF, "bare", { number: 1, tier: "S3" });
  await makeAttempt(WF, "bare", { number: 2, worktree: "/w" });
  const missing = (name: string) => (e: unknown) => e instanceof TemplateError && e.code === "template_missing_value" && e.details.name === name;
  await assert.rejects(renderWorkerSpec(WF, "bare-a1"), missing("worktree"));
  await assert.rejects(renderWorkerSpec(WF, "bare-a2"), missing("tier"));
  assert.ok(!existsSync(specPath(WF, "bare-a1")), "nothing is written on error");
  await assert.rejects(renderWorkerSpec(WF, "bare-a3"), (e: unknown) => e instanceof CpdError && e.code === "attempt_not_found" && e.exitCode === 4);
  await assert.rejects(renderWorkerSpec(WF, "ghost-a1"), (e: unknown) => e instanceof CpdError && e.code === "job_not_found");
  await assert.rejects(renderWorkerSpec(WF, "bad id"), (e: unknown) => e instanceof CpdError && e.code === "invalid_attempt_id");
  await assert.rejects(renderReviewBrief(WF, "bare-a1", { diffPath: "/d", gatePath: "/g", worktree: "/w" }), (e: unknown) => e instanceof CpdError && e.code === "spec_missing");
  await assert.rejects(renderReviewBrief(WF, "api-a2", { diffPath: "/d", gatePath: "/g", worktree: undefined as unknown as string }), (e: unknown) => e instanceof TemplateError && e.details.name === "worktree");
});

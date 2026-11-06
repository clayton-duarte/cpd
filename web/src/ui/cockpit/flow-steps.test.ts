import { readFileSync } from "node:fs";
import { expect, it } from "vitest";
import { FLOW_STEPS, forwardEdgeIds, loopEdgeIds } from "./flow-steps.ts";

type Row = [string, string, string, string, string | null, string | null];
const N = null;
const ROWS: Row[] = [
  ["01", "Intake raw input", "context", "manager", "Plan (shaping)", N],
  ["02", "Explore", "context", "manager", "explore/<job>.md", "02 · more explore"],
  ["03", "Grill the User", "context", "manager", "Context Records", "03 · ask again"],
  ["04", "Write the brief", "context", "manager", "Plan Brief", N],
  ["05", "Spawn the Lead", "context", "manager", N, N],
  ["06", "Ground", "context", "lead", N, N],
  ["07", "Clarify", "plan", "lead", "Assumptions A1..An", N],
  ["08", "Scope", "plan", "lead", "Workflows", "07 · new assumption"],
  ["09", "Split", "plan", "lead", "Job Specs", N],
  ["10", "Outline", "plan", "lead", "Plan Outline", N],
  ["11", "Review", "plan", "lead", N, N],
  ["12", "Lock", "plan", "lead", "Lead Branch <plan>/<wf>", N],
  ["13", "Queue later batches", "plan", "lead", N, "11 · re-review"],
  ["14", "Document", "dispatch", "worker", "Docs Commit", N],
  ["15", "Red tests", "dispatch", "worker", "Red Test Commit", N],
  ["16", "Implement", "dispatch", "worker", "Green Impl Commit", "16 · retry"],
  ["17", "Merge into Lead Branch", "dispatch", "lead", "Lead Branch Commit", N],
  ["18", "Open pull request", "dispatch", "lead", "Pull Request", "16 · fix gate or CI"],
  ["19", "Review and reply", "dispatch", "lead", "PR Review / Reply", "16 · fix review"],
  ["20", "Merge and close", "dispatch", "manager", "Merged PR (done)", "10 · follow-up work"],
];

const loopOf = (s: { loop: { to: string; label: string } | null }): string | null =>
  s.loop === null ? null : `${s.loop.to} · ${s.loop.label}`;

it("flow steps are numbered 01 to 20 in order", () => {
  expect(FLOW_STEPS.length).toBe(20);
  expect(FLOW_STEPS.map((s) => s.id)).toEqual(ROWS.map((r) => r[0]));
});

it("each flow step has the pinned name, stage, agent and artifact", () => {
  expect(FLOW_STEPS.length).toBe(20);
  expect(FLOW_STEPS.map((s) => [s.name, s.phase, s.agent, s.artifact])).toEqual(
    ROWS.map((r) => [r[1], r[2], r[3], r[4]]),
  );
});

it("loop steps are exactly 02 03 08 13 16 18 19 20 with their targets and labels", () => {
  expect(FLOW_STEPS.length).toBe(20);
  const loops = FLOW_STEPS.filter((s) => s.loop !== null).map((s) => [s.id, loopOf(s)]);
  expect(loops).toEqual(ROWS.filter((r) => r[5] !== null).map((r) => [r[0], r[5]]));
  expect(loops.map((l) => l[0]).join(" ")).toBe("02 03 08 13 16 18 19 20");
});

it("every flow step has a description and every loop a condition", () => {
  expect(FLOW_STEPS.length).toBe(20);
  for (const s of FLOW_STEPS) {
    expect(s.description.trim()).not.toBe("");
    if (s.loop !== null) expect(s.loop.condition.trim()).not.toBe("");
  }
});

it("edge ids are 19 forward and 8 loop ids in order", () => {
  expect(FLOW_STEPS.length).toBe(20);
  const fwd = Array.from({ length: 19 }, (_, i) => {
    const a = String(i + 1).padStart(2, "0");
    const b = String(i + 2).padStart(2, "0");
    return `${a}>${b}`;
  });
  expect(forwardEdgeIds()).toEqual(fwd);
  expect(loopEdgeIds()).toEqual([
    "loop:02>02",
    "loop:03>03",
    "loop:08>07",
    "loop:13>11",
    "loop:16>16",
    "loop:18>16",
    "loop:19>16",
    "loop:20>10",
  ]);
});

it("docs/plan-lifecycle.md step table matches FLOW_STEPS", () => {
  expect(FLOW_STEPS.length).toBe(20);
  const md = readFileSync(new URL("../../../../docs/plan-lifecycle.md", import.meta.url), "utf8");
  const dash = (v: string): string | null => (v === "—" ? null : v);
  const rows = md
    .split("\n")
    .filter((l) => /^\| (\d\d) \|/.test(l))
    .map((l) => l.split("|").slice(1, -1).map((c) => c.trim()))
    .map((c) => [c[0], c[1], c[2], c[3], dash(c[4] ?? ""), dash(c[5] ?? "")]);
  expect(rows.length).toBe(20);
  expect(FLOW_STEPS.map((s) => [s.id, s.name, s.phase, s.agent, s.artifact, loopOf(s)])).toEqual(rows);
});

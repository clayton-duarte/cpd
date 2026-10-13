import { loadJobs, type Job } from "../ledger/jobs.ts";
import { readLedger } from "../ledger/bd.ts";
import { planFingerprintOf } from "../ledger/plan.ts";
import { getPhase, readWorkflow } from "../ledger/workflow.ts";
import { planPath } from "../lib/paths.ts";
import { writeTextAtomic } from "./template.ts";

// The plan Markdown the User reviews in plannotator (contract §11 `plan render`).
// Only GFM, tables, code and Mermaid: no footnotes, no raw HTML (trial t4).

/** Inline text from the Ledger made safe for GFM: no HTML tags, no footnote syntax. Code spans are left alone. */
function inline(text: string): string {
  return text
    .split(/(`[^`\n]*`)/)
    .map((part, i) => (i % 2 === 1 ? part : part.replace(/<(?=[A-Za-z/!?])/g, "&lt;").replace(/\[\^/g, "\\[^")))
    .join("");
}

/** Block text (goal, context) made safe the same way, leaving fenced code blocks untouched. */
function block(text: string): string {
  let fence: string | null = null;
  return text
    .split("\n")
    .map((line) => {
      const m = /^ {0,3}(`{3,}|~{3,})/.exec(line);
      if (fence === null && m) fence = m[1]!;
      else if (fence !== null && m && m[1]!.startsWith(fence[0]!) && m[1]!.length >= fence.length) fence = null;
      else if (fence === null) return inline(line);
      return line;
    })
    .join("\n");
}

const cell = (text: string) => inline(text).replace(/\|/g, "\\|").replace(/\s*\n\s*/g, " ");
const code = (text: string) => (text.includes("`") ? `\`\` ${text} \`\`` : `\`${text}\``);
/** A code span inside a table cell: `|` must be escaped even there. */
const tcode = (text: string) => code(text).replace(/\|/g, "\\|");
const list = (items: string[]) => items.map((i) => `- ${inline(i).replace(/\n/g, "\n  ")}`).join("\n");

const sortedNeeds = (j: Job) => [...j.needs].sort();

/** Mermaid node ids are positional (`j1`, `j2`, …) because Job keys may be reserved words; the label is the key. */
function graph(jobs: Job[]): string {
  const ids = new Map(jobs.map((j, i) => [j.key, `j${i + 1}`]));
  const lines = ["graph TD", ...jobs.map((j) => `  ${ids.get(j.key)}["${j.key}"]`)];
  for (const j of jobs) for (const n of sortedNeeds(j)) lines.push(`  ${ids.get(n)} --> ${ids.get(j.key)}`);
  return ["```mermaid", ...lines, "```"].join("\n");
}

/** The Markdown for `jobs` (every Job of the Workflow, in creation order). Pure. */
export function planMarkdown(title: string, phase: string, jobs: Job[]): string {
  const out = [`# Plan: ${inline(title)}`, "", `Phase: \`${phase}\``, ""];
  if (jobs.length === 0) {
    out.push("No Jobs yet.", "");
    return out.join("\n");
  }
  out.push(
    "## Jobs",
    "",
    "| Key | Title | Tier | Needs | Touches |",
    "|---|---|---|---|---|",
    ...jobs.map((j) => `| ${tcode(j.key)} | ${cell(j.title)} | ${j.tier} | ${sortedNeeds(j).map(tcode).join(", ") || "none"} | ${j.touches.map(tcode).join(", ")} |`),
    "",
    "## Dependencies",
    "",
    graph(jobs),
    "",
  );
  for (const j of jobs) {
    out.push(
      `## ${code(j.key)}: ${inline(j.title)}`,
      "",
      `Status: \`${j.status}\` · Tier: \`${j.tier}\` · Review Tier: \`${j.reviewTier}\` · Needs: ${sortedNeeds(j).map(code).join(", ") || "none"}`,
      "",
      "### Goal",
      "",
      block(j.goal),
      "",
      "### Context",
      "",
      block(j.context) || "None.",
      "",
      "### Files it touches",
      "",
      j.touches.map((t) => `- ${code(t)}`).join("\n"),
      "",
      "### Acceptance",
      "",
      list(j.acceptance),
      "",
      "### Out of scope",
      "",
      j.outOfScope.length === 0 ? "None." : list(j.outOfScope),
      "",
    );
  }
  return out.join("\n");
}

/** Render the plan of `wf` to `out` (default `<wf>/plan.md`). The fingerprint is of the same Job snapshot the Markdown shows. */
export async function renderPlan(wf: string, out: string = planPath(wf)): Promise<{ path: string; fingerprint: string }> {
  const { title } = readWorkflow(wf);
  const phase = await getPhase(wf);
  const { jobs } = await loadJobs(readLedger(wf));
  writeTextAtomic(out, planMarkdown(title, phase, jobs));
  return { path: out, fingerprint: planFingerprintOf(jobs) };
}

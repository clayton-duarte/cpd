import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";

const root = new URL("../skills/", import.meta.url);

function files(skill: string): { name: string; text: string }[] {
  const dir = new URL(`${skill}/`, root);
  const refs = new URL("references/", dir);
  const names = existsSync(refs) ? readdirSync(refs).sort() : [];
  return [
    { name: `${skill}/SKILL.md`, text: readFileSync(new URL("SKILL.md", dir), "utf8") },
    ...names.map((f) => ({ name: `${skill}/references/${f}`, text: readFileSync(new URL(f, refs), "utf8") })),
  ];
}
const all = [...files("cpd-lead"), ...files("cpd-manager")];
const lead = files("cpd-lead")[0]!.text;
const manager = files("cpd-manager")[0]!.text;

/** The heading line to the next heading of the same or higher level. */
function section(text: string, heading: string): string {
  const level = /^#+/.exec(heading)![0].length;
  const lines = text.split("\n");
  const start = lines.indexOf(heading);
  assert.ok(start >= 0, `missing heading: ${heading}`);
  const end = lines.findIndex((l, i) => i > start && new RegExp(`^#{1,${level}} `).test(l));
  return lines.slice(start, end < 0 ? undefined : end).join("\n");
}

/** Every line of every file matching a pattern, as `file:line: text`. */
function hits(re: RegExp): string[] {
  const out: string[] = [];
  for (const f of all) f.text.split("\n").forEach((l, i) => { if (re.test(l)) out.push(`${f.name}:${i + 1}: ${l.slice(0, 100)}`); });
  return out;
}

function numbered(text: string): { n: number; line: string }[] {
  return text.split("\n").flatMap((l) => {
    const m = /^(\d+)\. /.exec(l);
    return m ? [{ n: Number(m[1]), line: l.slice(0, 80) }] : [];
  });
}

function description(text: string): string {
  const m = /^---\n([\s\S]*?)\n---\n/.exec(text);
  assert.ok(m, "frontmatter");
  const d = /^description: (.+)$/m.exec(m[1]!);
  assert.ok(d, "description field");
  return d[1]!;
}

test("skills carry no Context record id", () => {
  const h = hits(/\b(hs|pro)-[dnqap][0-9]+/);
  assert.deepEqual(h, [], `record ids:\n${h.join("\n")}`);
});

test("skills carry no ISO date and no leaked Worker instruction", () => {
  const h = hits(/\b20\d\d-\d\d-\d\d\b|on the ledger of|renumber the later steps|formerly|trial t5/);
  assert.deepEqual(h, [], `history:\n${h.join("\n")}`);
});

test("every step reference to Orchestrating names step 5 or step 7 and the steps run 1 to 7", () => {
  const want = ["Render", "Settle the overlaps", "Lint", "Write the outline", "Review", "Lock", "Add the later batches"];
  const steps = section(lead, "### Orchestrating").split("\n").flatMap((l) => {
    const m = /^(\d+)\. \*\*([^*]+?)\.?\*\*/.exec(l);
    return m ? [{ n: Number(m[1]), title: m[2]! }] : [];
  });
  assert.deepEqual(steps.map((s) => s.title), want, `Orchestrating steps: ${JSON.stringify(steps)}`);
  assert.deepEqual(steps.map((s) => s.n), [1, 2, 3, 4, 5, 6, 7], "Orchestrating step numbers");
  const bad: string[] = [];
  lead.split("\n").forEach((l, i) => {
    for (const m of l.matchAll(/\bstep (\d+)\b/g)) if (m[1] !== "5" && m[1] !== "7") bad.push(`cpd-lead/SKILL.md:${i + 1}: ${m[0]}`);
  });
  assert.deepEqual(bad, [], `step references:\n${bad.join("\n")}`);
});

test("the Addressing list is numbered 1 upward without a restart", () => {
  const items = numbered(section(lead, "#### Addressing"));
  assert.ok(items.length > 0, "Addressing has a numbered list");
  items.forEach((it, i) => assert.equal(it.n, i + 1, `Addressing item ${i + 1} is numbered ${it.n}: ${it.line}`));
});

test("Failed Jobs has nine cause rows and no dirty_target row", () => {
  const rows = section(lead, "#### Failed Jobs").split("\n").flatMap((l) => {
    const m = /^\| `([a-z_]+)`/.exec(l);
    return m ? [m[1]!] : [];
  });
  assert.deepEqual(
    rows,
    ["timed_out", "no_submit", "crashed", "merge_conflict", "gate_failed", "provider_error", "setup_failed", "cas_lost", "tampered"],
    `cause rows: ${rows.join(", ")}`,
  );
  const h = hits(/dirty_target/);
  assert.deepEqual(h, [], `dirty_target:\n${h.join("\n")}`);
});

test("frontmatter descriptions stay short and keep their Use when triggers", () => {
  const stages = /Grounding|Grilling|Clarifying|Scoping|Splitting|Orchestrating|Documenting/;
  const l = description(lead);
  const m = description(manager);
  assert.ok(l.length <= 330, `cpd-lead description is ${l.length} characters (max 330)`);
  assert.ok(m.length <= 260, `cpd-manager description is ${m.length} characters (max 260)`);
  for (const [name, d, tokens] of [["cpd-lead", l, ["Use when", "Lead", "Event"]], ["cpd-manager", m, ["Use when", "Manager", "role manager"]]] as const) {
    for (const t of tokens) assert.ok(d.includes(t), `${name} description lacks "${t}"`);
    assert.doesNotMatch(d, stages, `${name} description lists stages`);
  }
  assert.ok(!m.includes("splits the User's requirements into Plans"), "cpd-manager description says it splits the User's requirements into Plans");
});

test("Grilling Interview Grounding and Clarifying end on checkable completion criteria", () => {
  const miss = (where: string, text: string, tokens: string[]) =>
    tokens.filter((t) => !text.toLowerCase().includes(t.toLowerCase())).map((t) => `${where} lacks "${t}"`);
  const item = (text: string, re: RegExp, where: string) => {
    const l = text.split("\n").find((x) => re.test(x));
    assert.ok(l, `${where}: item not found`);
    return l;
  };
  const grill = item(section(lead, "### Grilling"), /^1\. /, "cpd-lead Grilling item 1");
  const clar = item(section(lead, "### Clarifying"), /^1\. /, "cpd-lead Clarifying item 1");
  const problems = [
    ...(grill.includes("minimised") ? ["cpd-lead Grilling item 1 still says minimised"] : []),
    ...miss("cpd-lead Grilling item 1", grill, ["public seam", "error case", "acceptance check", "without a guess", "every open decision"]),
    ...miss("cpd-lead Grounding", section(lead, "### Grounding"), ["done when", "every touched repo", "every file path"]),
    ...miss("cpd-lead Clarifying item 1", clar, ["every deliverable", "affected repo", "out-of-scope"]),
    ...miss("cpd-manager Intake", item(manager, /^1\. \*\*Intake\.\*\*/, "Intake"), ["every deliverable", "affected repo", "out-of-scope"]),
    ...miss("cpd-manager Interview", item(manager, /^2\. \*\*Interview/, "Interview"), ["every Requirement", "repo boundary", "Plan cut", "cross-Plan"]),
  ];
  assert.deepEqual(problems, [], problems.join("\n"));
});

const words = (t: string) => t.split(/\s+/).filter(Boolean).length;
const lineOf = (text: string, needle: string) => text.split("\n").findIndex((l) => l.includes(needle)) + 1;
const bulletOf = (text: string, start: string) => text.split("\n").find((l) => l.startsWith(start)) ?? assert.fail(`no line starting ${start}`);
const specFiles = files("cpd-lead").find((f) => f.name.endsWith("references/spec-files.md"))!.text;

test("the Register planned work bullet states the draft rule once in at most 180 words", () => {
  const b = bulletOf(lead, "- **Register planned work at once.**");
  const line = lineOf(lead, "- **Register planned work at once.**");
  assert.ok(words(b) <= 180, `cpd-lead/SKILL.md:${line}: the Register bullet has ${words(b)} words, at most 180`);
  for (const token of ["cpd job add <k> --draft --title", "--needs <k1,k2>", "draft_parked", "draft_incomplete", "stub", "under review"]) {
    assert.ok(b.includes(token), `cpd-lead/SKILL.md:${line}: the Register bullet lacks ${token}`);
  }
  assert.match(b, /decided work is (added )?queued/i, `cpd-lead/SKILL.md:${line}: the bullet must say decided work is added queued`);
  const d = bulletOf(lead, "- **Decided work is queued, undecided design is a draft.**");
  assert.ok(d.includes("Spec that cites the decision") && d.includes("When unsure, a draft"), "the Decided work bullet keeps its two phrases");
});

test("Splitting Orchestrating and Starting a later batch point to Rules instead of restating draft legality", () => {
  const lines = lead.split("\n");
  const s7 = lines.findIndex((l) => l.startsWith("7. **Add the later batches**"));
  const d3 = lines.findIndex((l) => l === "## 3. Dispatch");
  assert.ok(s7 >= 0 && d3 > s7, "step 7 and ## 3. Dispatch exist");
  const blocks = [
    { name: "Plan every batch now", line: lineOf(lead, "- **Plan every batch now.**"), text: bulletOf(lead, "- **Plan every batch now.**"), max: 90 },
    { name: "Orchestrating step 7", line: s7 + 1, text: lines.slice(s7, d3).join("\n"), max: 200 },
    { name: "Starting a later batch", line: lineOf(lead, "#### Starting a later batch"), text: section(lead, "#### Starting a later batch"), max: 180 },
  ];
  const bad = blocks.flatMap((b) => [
    ...(words(b.text) > b.max ? [`cpd-lead/SKILL.md:${b.line}: ${b.name} has ${words(b.text)} words, at most ${b.max}`] : []),
    ...(b.text.includes("](#rules)") ? [] : [`cpd-lead/SKILL.md:${b.line}: ${b.name} has no link ](#rules)`]),
  ]);
  assert.deepEqual(bad, [], bad.join("\n"));
});

test("Exploring keeps the step and command and points to spec-files for the refusals", () => {
  const e = section(lead, "#### Exploring");
  const line = lineOf(lead, "#### Exploring");
  const bad: string[] = [];
  if (words(e) > 220) bad.push(`cpd-lead/SKILL.md:${line}: Exploring has ${words(e)} words, at most 220`);
  if (!e.includes('cpd job add <k> --title "<title>" --tier <T> --spec <file.json> --kind scout|research')) bad.push(`cpd-lead/SKILL.md:${line}: Exploring lacks the job add --kind command`);
  if (!e.includes("references/spec-files.md#explore-jobs")) bad.push(`cpd-lead/SKILL.md:${line}: Exploring lacks the link to references/spec-files.md#explore-jobs`);
  const codes = ["explore_workflow_forbidden", "explore_no_draft", "explore_needs_mixed", "tier_not_allowed", "illegal_phase"];
  for (const c of codes) if (e.includes(c)) bad.push(`cpd-lead/SKILL.md:${lineOf(lead, c)}: Exploring names ${c}`);
  const x = section(specFiles, "## Explore Jobs");
  for (const c of codes) if (!x.includes(c)) bad.push(`spec-files.md ## Explore Jobs lacks ${c}`);
  assert.deepEqual(bad, [], bad.join("\n"));
});

test("spec-files tells a Research Job from an explore Job", () => {
  const r = section(specFiles, "## Research Jobs");
  const x = section(specFiles, "## Explore Jobs");
  const bad: string[] = [];
  const rl = lineOf(specFiles, "## Research Jobs");
  if (!r.includes("ordinary Workflow Job")) bad.push(`spec-files.md:${rl}: Research Jobs does not say ordinary Workflow Job`);
  if (!r.includes("docs/research/<topic>.md")) bad.push(`spec-files.md:${rl}: Research Jobs lacks docs/research/<topic>.md`);
  if (!r.includes("](#explore-jobs)")) bad.push(`spec-files.md:${rl}: Research Jobs lacks a link to #explore-jobs`);
  const xl = lineOf(specFiles, "## Explore Jobs");
  if (!x.includes("--kind scout|research")) bad.push(`spec-files.md:${xl}: Explore Jobs lacks --kind scout|research`);
  if (!x.includes("$CPD_HOME/run/<plan>/explore/<job-key>.md")) bad.push(`spec-files.md:${xl}: Explore Jobs lacks the artefact $CPD_HOME/run/<plan>/explore/<job-key>.md`);
  assert.deepEqual(bad, [], bad.join("\n"));
});

test("adopted Workflow reminders outside Scoping and Addressing are one-line pointers", () => {
  const bad: string[] = [];
  const scoping = section(lead, "### Scoping");
  for (const t of ["--adopt", "adopt_foreign", "adopt_not_open", "adopt_fork", "adopt_cpd_branch", "adopt_taken", "--confirm-author"]) {
    if (!scoping.includes(t)) bad.push(`### Scoping lacks ${t}`);
  }
  const addr = section(lead, "#### Addressing");
  if (!addr.includes("cpd pr rebase --workflow <key>")) bad.push("#### Addressing lacks cpd pr rebase --workflow <key>");
  if (!addr.includes("never a plain `git push`")) bad.push("#### Addressing lacks never a plain `git push`");
  const lines = lead.split("\n");
  const inRange = (i: number) => {
    const sIdx = lines.indexOf("### Scoping"), aIdx = lines.indexOf("#### Addressing");
    return (i >= sIdx && i < sIdx + scoping.split("\n").length) || (i >= aIdx && i < aIdx + addr.split("\n").length);
  };
  lines.forEach((l, i) => {
    if (inRange(i)) return;
    for (const s of l.split(/(?<=[.!?])\s+/).filter((x) => /adopted/i.test(x))) {
      const why = [
        ...(words(s) > 30 ? [`${words(s)} words`] : []),
        ...(/\]\(#(scoping|addressing)\)/.test(s) ? [] : ["no link to #scoping or #addressing"]),
        ...(s.includes("cpd pr rebase") ? ["names cpd pr rebase"] : []),
      ];
      if (why.length) bad.push(`cpd-lead/SKILL.md:${i + 1}: ${why.join(", ")}: ${s.slice(0, 70)}`);
    }
  });
  for (const f of all.filter((f) => f.name !== "cpd-lead/SKILL.md")) f.text.split("\n").forEach((l, i) => {
    for (const s of l.split(/(?<=[.!?])\s+/).filter((x) => /adopted/i.test(x))) {
      if (words(s) > 30 || !/\]\(#(scoping|addressing)\)/.test(s)) bad.push(`${f.name}:${i + 1}: adopted sentence is not a pointer`);
    }
  });
  assert.deepEqual(bad, [], bad.join("\n"));
});

const rulesBullets = (text: string): { n: number; line: string }[] => {
  const rules = section(text, "## Rules");
  const base = text.split("\n").indexOf("## Rules") + 1;
  return rules.split("\n").flatMap((line, i) => (line.startsWith("- ") ? [{ n: base + i, line }] : []));
};
const count = (text: string, re: RegExp) => (text.match(re) ?? []).length;

test("no Rules bullet of either skill starts with Never or Do not", () => {
  const bad: string[] = [];
  for (const [name, text] of [["cpd-lead", lead], ["cpd-manager", manager]] as const)
    for (const b of rulesBullets(text))
      if (/^- (\*\*)?(never|do not)\b/i.test(b.line)) bad.push(`${name}/SKILL.md:${b.n}: ${b.line.slice(0, 90)}`);
  // R17 kept: the command lists and the git list stay word for word.
  const bulletWith = (text: string, needles: string[]) => rulesBullets(text).some((b) => needles.every((n) => b.line.includes(n)));
  const cpdOwn = ["`cpd plan decide`", "`cpd watch`", "`cpd attempt run`", "`cpd job dispatch`", "`cpd session run`", "`cpd tui`", "`cpd events follow`"];
  const leadOwn = ["`cpd plan phase plan`", "`cpd workflow add`", "`cpd job add`", "`cpd plan review`", "`cpd plan lock`", "`cpd answer`", "`cpd job approve-tests`"];
  const git = ["checkout, switch, reset, rebase, merge, pull, stash"];
  assert.ok(bulletWith(lead, cpdOwn), "cpd-lead keeps the cpd-owned command list");
  assert.ok(bulletWith(manager, [...cpdOwn, "`cpd context manager`"]), "cpd-manager keeps the cpd-owned command list");
  assert.ok(bulletWith(manager, leadOwn), "cpd-manager keeps the Lead-owned command list");
  assert.ok(bulletWith(lead, git), "cpd-lead keeps the git list");
  assert.ok(bulletWith(manager, git), "cpd-manager keeps the git list");
  assert.deepEqual(bad, [], `Rules bullets that start with a negation:\n${bad.join("\n")}`);
});

test("negation stays under the ratchet in both skills and spec-files", () => {
  const spec = files("cpd-lead").find((f) => f.name.endsWith("references/spec-files.md"))!.text;
  const re = /\b(never|do not|don't)\b/gi;
  const got = { lead: count(lead, re), manager: count(manager, re), spec: count(spec, re) };
  const over: string[] = [];
  if (got.lead > 86) over.push(`cpd-lead/SKILL.md has ${got.lead} negations, at most 86`);
  if (got.manager > 31) over.push(`cpd-manager/SKILL.md has ${got.manager} negations, at most 31`);
  if (got.spec > 20) over.push(`cpd-lead/references/spec-files.md has ${got.spec} negations, at most 20`);
  assert.deepEqual(over, [], over.join("; "));
});

test("ready_to_post is one chat line and cpd pr post waits for the User, stated once", () => {
  const problems: string[] = [];
  const review = section(lead, "#### Pull request review");
  const lineOf = (text: string, s: string) => text.split("\n").findIndex((l) => l.toLowerCase().includes(s.toLowerCase())) + 1;
  if (!/one chat line/i.test(review)) problems.push("#### Pull request review lacks `one chat line`");
  if (!review.includes("cpd pr post --workflow <key>") || !/only when the User/.test(review)) problems.push("#### Pull request review lacks `cpd pr post --workflow <key>` ... `only when the User`");
  const n = count(lead, /one chat line/gi);
  if (n !== 1) problems.push(`\`one chat line\` occurs ${n} times in cpd-lead/SKILL.md (first at line ${lineOf(lead, "one chat line")}), want 1`);
  const row = lead.split("\n").find((l) => l.startsWith("| `pr-feedback` |")) ?? "";
  if (!row.includes("](#pull-request-review)")) problems.push(`the pr-feedback row (line ${lineOf(lead, "| `pr-feedback` |")}) lacks a link to #pull-request-review`);
  if (!section(lead, "#### Posting for review").includes("](#pull-request-review)")) problems.push(`#### Posting for review (line ${lineOf(lead, "#### Posting for review")}) lacks a link to #pull-request-review`);
  for (const s of ["never ask whether to post", "never post and never ask"]) if (lead.toLowerCase().includes(s)) problems.push(`\`${s}\` at line ${lineOf(lead, s)}`);
  assert.deepEqual(problems, [], problems.join("\n"));
});

test("the session and read-only rules lead with their positive target", () => {
  const problems: string[] = [];
  const lb = rulesBullets(lead);
  for (const b of lb) if (b.line.startsWith("- Never review inside")) problems.push(`cpd-lead/SKILL.md:${b.n}: starts with Never review inside`);
  for (const b of lb) if (b.line.startsWith("- **Never review inside")) problems.push(`cpd-lead/SKILL.md:${b.n}: starts with **Never review inside`);
  const keep = lb.find((b) => b.line.startsWith("- **Keep your session"));
  if (!keep) problems.push("cpd-lead Rules has no bullet starting `- **Keep your session`");
  else if (!keep.line.includes("explore Job")) problems.push(`cpd-lead/SKILL.md:${keep.n}: the Keep your session bullet lacks \`explore Job\``);
  for (const [name, text] of [["cpd-lead", lead], ["cpd-manager", manager]] as const) {
    const stay = rulesBullets(text).find((b) => b.line.startsWith("- **Stay free for the User.**"));
    if (!stay) problems.push(`${name} Rules has no bullet starting \`- **Stay free for the User.**\``);
    else for (const w of ["30 seconds", "background"]) if (!stay.line.includes(w)) problems.push(`${name}/SKILL.md:${stay.n}: Stay free bullet lacks \`${w}\``);
  }
  const nb = manager.split("\n").findIndex((l) => l.includes("**Never block.**")) + 1;
  if (nb) problems.push(`cpd-manager/SKILL.md:${nb}: still has **Never block.**`);
  if (!lead.includes("Events arrive as new turns")) problems.push("cpd-lead lacks `Events arrive as new turns`");
  assert.deepEqual(problems, [], problems.join("\n"));
});

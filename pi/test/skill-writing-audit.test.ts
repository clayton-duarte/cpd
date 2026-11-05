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
  for (const token of ["--needs <k1,k2>", "draft_parked", "draft_incomplete", "under review"]) {
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

const prDescription = files("cpd-lead").find((f) => f.name.endsWith("pr-description.md"))!.text;

/** The lines of a text that match, as `line N: text`. */
function lineHits(text: string, re: RegExp): string[] {
  return text.split("\n").flatMap((l, i) => (re.test(l) ? [`line ${i + 1}: ${l.slice(0, 100)}`] : []));
}

test("Addressing keeps six steps and drops the plan-done and register repeats", () => {
  const sec = section(lead, "#### Addressing");
  const items = numbered(sec);
  assert.deepEqual(items.map((i) => i.n), [1, 2, 3, 4, 5, 6], `Addressing items: ${items.map((i) => i.line).join(" | ")}`);
  const bad = lineHits(sec, /The Plan is done when every Workflow is merged|Register every further fix/);
  assert.deepEqual(bad, [], `repeats in Addressing:\n${bad.join("\n")}`);
  const row = lead.split("\n").find((l) => l.startsWith("| `plan-done`"))!;
  assert.ok(row, "plan-done row");
  assert.ok(row.includes('cpd workflow add <key> --repo <path> --title "<title>"'), `plan-done row lacks the workflow add command: ${row.slice(0, 100)}`);
  assert.ok(/--workflow <key>/.test(row), `plan-done row lacks --workflow <key>: ${row.slice(0, 100)}`);
});

test("no sleep command and no opens only when you do clause in the skills", () => {
  const sleep = all.filter((f) => f.name.startsWith("cpd-lead/")).flatMap((f) => lineHits(f.text, /sleep 10/).map((h) => `${f.name} ${h}`));
  assert.deepEqual(sleep, [], `sleep 10:\n${sleep.join("\n")}`);
  const opens = all.flatMap((f) => lineHits(f.text, /opens only when you do/).map((h) => `${f.name} ${h}`));
  assert.deepEqual(opens, [], `opens only when you do:\n${opens.join("\n")}`);
});

test("Failed Jobs states the Gate retry once in its table row", () => {
  const sec = section(lead, "#### Failed Jobs");
  const bullets = lineHits(sec, /^- A `gate_failed` Attempt was already retried once/);
  assert.deepEqual(bullets, [], `repeated bullet:\n${bullets.join("\n")}`);
  const row = sec.split("\n").find((l) => l.startsWith("| `gate_failed`"))!;
  assert.ok(row && /retry/.test(row), `gate_failed row states the retry: ${row}`);
});

test("Rules merges the adjacent bullets and drops the ask only sentence and parenthesis misplacement", () => {
  const sec = section(lead, "## Rules");
  const split = lineHits(sec, /^- \*\*Ask the User directly\.\*\*/);
  assert.deepEqual(split, [], `bullet not merged into Resolve first:\n${split.join("\n")}`);
  assert.ok(/^- \*\*Resolve first, ask second\.\*\*/m.test(sec), "Resolve first bullet stays");
  const sentence = lineHits(sec, /Ask the User only for decisions you cannot derive/);
  assert.deepEqual(sentence, [], `sentence stays:\n${sentence.join("\n")}`);
  const forbidden = sec.split("\n").filter((l) => l.includes("agent_forbidden"));
  assert.ok(forbidden.length > 0, "agent_forbidden stays in Rules");
  for (const l of forbidden) {
    assert.ok(!l.includes("cpd plan decide"), `agent_forbidden stands beside cpd plan decide: ${l.slice(0, 100)}`);
    for (const c of ["cpd context archive", "cpd context unarchive", "cpd plan archive", "cpd plan unarchive", "cpd session archive", "cpd session unarchive"])
      assert.ok(l.includes(c), `agent_forbidden line lacks ${c}: ${l.slice(0, 100)}`);
  }
});

test("Agent keys names the three keys only", () => {
  for (const [name, text] of [["cpd-lead", lead], ["cpd-manager", manager]] as const) {
    const sec = section(text, "## Agent keys");
    for (const k of ["`<context>`", "`<context>:<plan>`", "`<context>:<plan>:<job>`", "A key is fixed"])
      assert.ok(sec.includes(k), `${name} Agent keys lacks ${k}`);
    const bad = lineHits(text, /<plan>\.<prefix>|UUID/);
    assert.deepEqual(bad, [], `${name} names another address:\n${bad.join("\n")}`);
  }
});

test("Exploring and Test Jobs drop the repeats of their reference", () => {
  const promo = lineHits(section(lead, "#### Exploring"), /docs\/research/);
  assert.deepEqual(promo, [], `Exploring restates the promotion:\n${promo.join("\n")}`);
  assert.ok(/Promoting an explore finding[^\n]*docs\/research/.test(specFiles), "spec-files keeps the promotion");
  const tdd = lineHits(section(specFiles, "## Test Jobs"), /tests_unconfigured/);
  assert.deepEqual(tdd, [], `Test Jobs restates the tdd template:\n${tdd.join("\n")}`);
  assert.ok(section(lead, "### Scoping").includes("tests_unconfigured"), "Scoping keeps tests_unconfigured");
});

test("every body.md placeholder is closed", () => {
  const open = all.flatMap((f) => lineHits(f.text, /<body\.md(?!>)/).map((h) => `${f.name} ${h}`));
  assert.deepEqual(open, [], `unclosed placeholder:\n${open.join("\n")}`);
});

test("pr-description names a Workflow key and reads clearly", () => {
  const good = prDescription.slice(prDescription.indexOf("## Good example"));
  const start = good.indexOf("## Out of scope and follow-ups\n");
  assert.ok(start >= 0, "good example has the Out of scope section");
  const out = good.slice(start, good.indexOf("## Links", start));
  assert.ok(/\bWorkflow [a-z][a-z0-9]*(-[a-z0-9]+)*\b/.test(out), `good example names no Workflow key: ${out.replace(/\n/g, " ").slice(0, 200)}`);
  const rule = prDescription.split("\n").find((l) => l.startsWith("**What and why:**"))!;
  assert.ok(rule, "What and why rule");
  assert.ok(!/questions answered/.test(rule), `garbled sentence: ${rule.slice(0, 260)}`);
});

test("Splitting and Grilling drop the unsourced figure and the repeats", () => {
  const split = lineHits(section(lead, "### Splitting"), /150 turns|finish without asking/);
  assert.deepEqual(split, [], `Splitting:\n${split.join("\n")}`);
  const size = lineHits(section(specFiles, "## Size"), /150 turns/);
  assert.deepEqual(size, [], `spec-files Size:\n${size.join("\n")}`);
  const item = section(lead, "### Grilling").split("\n").find((l) => l.startsWith("1. "))!;
  assert.ok(item, "Grilling item 1");
  assert.ok(/group[^\n]*\(?references\/questions\.md|references\/questions\.md[^\n]*group/i.test(item), `Grilling item 1 does not link questions.md for grouping: ${item.slice(0, 120)}`);
  assert.ok(!/Group independent questions/.test(item), `Grilling item 1 restates the grouping rule: ${item.slice(0, 120)}`);
  assert.ok(/whole frontier at once/.test(files("cpd-lead").find((f) => f.name.endsWith("questions.md"))!.text), "questions.md keeps the grouping rule");
});

test("Within the Plan and the registered work list match the Manager", () => {
  const within = lead.split("\n").find((l) => l.startsWith("- **Within the Plan:**"))!;
  assert.ok(within, "Within the Plan bullet");
  assert.ok(within.includes("moves the work or re-routes it"), `Within the Plan lacks the Manager's wording: ${within.slice(0, 260)}`);
  assert.ok(!/handles the sequencing/.test(within), `Within the Plan says the Manager sequences: ${within.slice(0, 260)}`);
  const reg = manager.split("\n").find((l) => l.includes("Check each Plan's registered work"))!;
  assert.ok(reg, "registered work bullet");
  assert.ok(reg.includes("`queued`, `running` or `failed`"), `registered work list omits failed: ${reg.slice(0, 300)}`);
});

test("Manager Plans keeps six steps with the pending check in Intake", () => {
  const sec = section(manager, "## 2. Plans");
  const steps = sec.split("\n").filter((l) => /^\d+\. /.test(l));
  assert.deepEqual(steps.map((l) => Number(/^(\d+)/.exec(l)![1])), [1, 2, 3, 4, 5, 6], `steps: ${steps.map((l) => l.slice(0, 50)).join(" | ")}`);
  const intake = steps[0]!;
  assert.ok(/^1\. \*\*Intake/.test(intake), `step 1: ${intake.slice(0, 50)}`);
  for (const k of ["every deliverable", "affected repo", "out-of-scope"]) assert.ok(intake.includes(k), `Intake lacks ${k}`);
  assert.ok(intake.includes("cpd context pending --context <c>"), "Intake lacks the pending check");
  const late = steps.slice(1).filter((l) => l.includes("cpd context pending --context <c>"));
  assert.deepEqual(late, [], `pending check outside Intake: ${late.map((l) => l.slice(0, 60)).join(" | ")}`);
});

test("cpd-lead sends codebase reads to scout Jobs", () => {
  const bad: string[] = [];
  const phrases = ["by reading it yourself", "verified in source", "not cheaper to read", "cheap to answer by reading", "read instead"];
  for (const f of files("cpd-lead"))
    f.text.split("\n").forEach((l, i) => {
      for (const p of phrases) if (l.includes(p)) bad.push(`${f.name}:${i + 1}: contains "${p}"`);
    });
  const grounding = section(lead, "### Grounding");
  for (const t of ["scout", "research", "job-merged", "explore_pending"]) if (!grounding.includes(t)) bad.push(`### Grounding lacks ${t}`);
  for (const w of ["end", "turn"]) if (!new RegExp(`\\b${w}\\b`).test(grounding)) bad.push(`### Grounding lacks the word ${w}`);
  const grilling = section(lead, "### Grilling");
  if (!/cpd plan phase plan[^\n]*(only )?after[^\n]*explore[^\n]*merged|only after[^\n]*explore[^\n]*merged[^\n]*cpd plan phase plan|explore[^\n]*merged[^\n]*cpd plan phase plan/i.test(grilling))
    bad.push("### Grilling does not say `cpd plan phase plan` runs only after the explore Jobs merged");
  assert.deepEqual(bad, [], bad.join("\n"));
});

test("cpd-lead: a new User request on a done Plan is handled in the Plan, grounded by an explore Job, never routed to the Manager", () => {
  const sec = section(lead, "### Documenting, Red testing, Implementing, Verifying");
  const para = sec.split(/\n\s*\n/).flatMap((p) => p.split(/\n(?=- )/)).find((p) => p.replace(/^- /, "").startsWith("**A new request on a done Plan"));
  assert.ok(para, "no bullet or paragraph starting with **A new request on a done Plan");
  for (const token of ["scout", "research", "cpd workflow add", "dispatch"]) assert.ok(para.includes(token), `the done-Plan paragraph lacks ${token}`);
  assert.match(para, /never[^.]*Manager|Manager[^.]*never/i, "the paragraph must refuse routing the request to the Manager");
});

test("cpd-lead: Exploring lists done among the phases an explore Job may be added in", () => {
  const e = section(lead, "#### Exploring");
  assert.match(e, /Add one in `grounding`, `plan`, `dispatch`, `deploy` or `done`/);
  assert.ok(words(e) <= 220, `Exploring has ${words(e)} words, at most 220`);
});

const refSpec = files("cpd-lead").find((f) => f.name.endsWith("references/spec-files.md"))!.text;
const docsDir = new URL("../../docs/", import.meta.url);
const docText = (n: string) => readFileSync(new URL(n, docsDir), "utf8");

test("draft is never a waiting state in the cpd-lead skill", () => {
  assert.match(lead, /being edited/, "a draft exists only while being edited");
  assert.match(lead, /pending (Plan )?review/, "or while covered by a pending review");
  assert.doesNotMatch(lead, /--draft --title/, "no stub draft form");
  assert.doesNotMatch(lead, /a stub \(/, "no stub description");
  assert.doesNotMatch(lead, /gets the stubs/, "no stubs from workflow add");
});

test("cpd-lead and cpd-manager never allow a stub draft that waits", () => {
  const texts = [...all.map((f) => ({ name: f.name, text: f.text })), { name: "spec-files.md", text: refSpec }, { name: "docs/contract.md", text: docText("contract.md") }, { name: "docs/commands.md", text: docText("commands.md") }];
  for (const { name, text } of texts) {
    for (const [i, l] of text.split("\n").entries()) {
      assert.doesNotMatch(l, /--draft --title/, `${name}:${i + 1}: stub draft form`);
      if (!name.startsWith("docs/")) assert.doesNotMatch(l, /gets the stubs|stub drafts?\b|a stub \(/i, `${name}:${i + 1}: stub draft allowed`);
      assert.doesNotMatch(l, /draft[^.\n]*\bwaits? on\b[^.\n]*(unfinished|queued|running|failed)|waiting draft/i, `${name}:${i + 1}: waiting draft allowed`);
    }
  }
});

test("waiting work is queued with needs and missing information is a scout or research Job", () => {
  for (const t of ["--needs", "scout", "research", "ask_user_question", "cpd_plan_review"]) assert.ok(lead.includes(t), `cpd-lead lacks ${t}`);
  const b = bulletOf(lead, "- **Register planned work at once.**");
  for (const t of ["--needs", "scout", "research", "ask_user_question", "cpd_plan_review"]) assert.ok(b.includes(t), `the Register rule lacks ${t}`);
});

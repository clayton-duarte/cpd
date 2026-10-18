import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";

const here = new URL("./", import.meta.url);
const commandsDoc = readFileSync(new URL("../../docs/commands.md", here), "utf8");

/** The skills of the package, each with the fewest `cpd …` commands it must name (a floor against a parser that finds nothing). */
const skills = [
  { skill: "cpd-lead", minCommands: 40 },
  { skill: "cpd-manager", minCommands: 15 },
] as const;

/** SKILL.md plus every file under references/ (if any): the whole text the agent can load. */
function skillFiles(skill: string): { name: string; text: string }[] {
  const skillDir = new URL(`${skill}/`, here);
  const refsDir = new URL("references/", skillDir);
  const refs = existsSync(refsDir) ? readdirSync(refsDir).sort() : [];
  return [
    { name: `${skill}/SKILL.md`, text: readFileSync(new URL("SKILL.md", skillDir), "utf8") },
    ...refs.map((f) => ({ name: `${skill}/references/${f}`, text: readFileSync(new URL(f, refsDir), "utf8") })),
  ];
}

for (const { skill } of skills) test(`${skill} frontmatter: its name and a description that says when to use it`, () => {
  const m = /^---\n([\s\S]*?)\n---\n/.exec(skillFiles(skill)[0]!.text);
  assert.ok(m, "SKILL.md must start with a --- frontmatter block");
  const fields: Record<string, string> = {};
  for (const line of m[1]!.split("\n")) {
    const kv = /^([a-z][a-z-]*): (.+)$/.exec(line);
    assert.ok(kv, `frontmatter line is not "key: value": ${line}`);
    assert.ok(!(kv[1]! in fields), `duplicate frontmatter key: ${kv[1]}`);
    fields[kv[1]!] = kv[2]!;
  }
  assert.deepEqual(Object.keys(fields).sort(), ["description", "name"]);
  assert.equal(fields.name, skill);
  assert.ok(fields.description!.length <= 1024, "pi allows descriptions up to 1024 characters");
  assert.match(fields.description!, /\bUse when\b/);
});

// --- the command lines the skill names must exist in commands.md ---

/** Words of a command line: the tokens up to the first argument (`<x>`, `[x]`, a flag, a quoted or `$` value). */
function words(line: string): string[] {
  const out: string[] = [];
  for (const token of line.trim().split(/\s+/)) {
    if (/^[<\["$(-]/.test(token)) break;
    out.push(token);
  }
  return out;
}

/** commands.md's synopses (the `cpd …` lines of its code blocks): words → every flag any synopsis of those words has. */
function synopses(): Map<string, Set<string>> {
  const map = new Map<string, Set<string>>();
  let fence: string | null = null; // the info string of the open fenced block
  for (const line of commandsDoc.split("\n")) {
    if (line.startsWith("```")) {
      fence = fence === null ? line.slice(3) : null;
      continue;
    }
    if (fence !== "" || !line.startsWith("cpd ")) continue;
    const key = words(line.slice("cpd ".length)).join(" ");
    const flags = map.get(key) ?? new Set<string>();
    for (const f of line.matchAll(/--[a-z][a-z-]*/g)) flags.add(f[0]);
    map.set(key, flags);
  }
  return map;
}

/** Every `cpd …` command the text names: inline code spans and lines of fenced blocks, indented or not (not the `cpd adapter:` message prefix). */
function commandsNamed(text: string): string[] {
  const found: string[] = [];
  const fenced = text.replace(/```[^\n]*\n([\s\S]*?)```/g, (_all, body: string) => {
    for (const line of body.split("\n")) if (line.trim().startsWith("cpd ")) found.push(line.trim()); // a block in a list item is indented
    return "";
  });
  for (const span of fenced.matchAll(/`([^`\n]+)`/g)) {
    if (span[1]!.startsWith("cpd ") && span[1] !== "cpd adapter:") found.push(span[1]!);
  }
  return found;
}

test("commands.md synopses are parsed (anchors)", () => {
  const known = synopses();
  for (const anchor of ["plan new", "plan phase plan", "job add", "answer", "session fork", "pr open", "watch"]) {
    assert.ok(known.has(anchor), `synopsis not parsed: ${anchor}`);
  }
  assert.ok(known.get("job add")!.has("--needs"));
});

for (const { skill, minCommands } of skills) test(`every cpd command ${skill} names exists in commands.md, with the flags used`, () => {
  const known = synopses();
  const named = skillFiles(skill).flatMap((f) => commandsNamed(f.text).map((c) => ({ file: f.name, command: c })));
  assert.ok(named.length > minCommands, `expected many commands, found ${named.length}`);
  for (const { file, command } of named) {
    const key = words(command.slice("cpd ".length)).join(" ");
    const flags = known.get(key);
    assert.ok(flags, `${file}: "${command}": "cpd ${key}" is not a command of commands.md`);
    for (const f of command.matchAll(/--[a-z][a-z-]*/g)) {
      assert.ok(flags.has(f[0]), `${file}: "${command}": ${f[0]} is not a flag of "cpd ${key}"`);
    }
  }
});

for (const { skill } of skills) test(`${skill}: nothing of cpd 1, of the removed Reviewer or of the retired grounded Lead is left`, () => {
  for (const { name, text } of skillFiles(skill)) {
    assert.doesNotMatch(text, /\bgrounded\b|forked_from/i, `${name}: the grounded Lead is retired (hs-d129); a Lead starts fresh`);
    assert.doesNotMatch(text, /--wf\b|\bCPD_WF\b/, `${name}: --wf is cpd 1`);
    assert.doesNotMatch(text, /\bworkflows?\b/i, `${name}: the term Workflow is retired`);
    assert.doesNotMatch(text, /herdr|\bbd\b|queue message|reviewMode|review-tier|review-needed|review decide|outOfScope/i, `${name}: cpd 1 or removed Reviewer text`);
  }
});

for (const { skill } of skills) test(`${skill}: relative links resolve to files of the skill`, () => {
  const files = new Set(skillFiles(skill).map((f) => f.name));
  for (const { name, text } of skillFiles(skill)) {
    for (const link of text.matchAll(/\]\((references\/[^)#]+)(?:#[^)]*)?\)/g)) {
      assert.ok(files.has(`${skill}/${link[1]!}`), `${name}: broken link ${link[1]}`);
    }
  }
});

test("cpd-lead rules allow git and gh for diagnosis, still forbid tmux, wt, force pushes and product commits", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  const rule = text.split("\n").find((l) => l.startsWith("- `git` and `gh`") || /`git` and `gh` are allowed/.test(l));
  assert.ok(rule, "Rules must say what git and gh may do");
  assert.match(rule, /Never run `tmux` or `wt`/);
  assert.match(rule, /never force-push/);
  assert.match(rule, /never commit product changes/);
  assert.ok(!/Never run `git`/.test(text), "git is no longer forbidden");
  assert.match(text, /gh pr checks <n>[^|\n]*gh run view <run-id> --log-failed/);
});

test("cpd-lead talks to its Manager with session send and still asks the User directly", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  assert.match(text, /cpd session send <manager-id>/);
  assert.match(text, /cpd session list --role manager/);
  assert.match(text, /session-message/);
  assert.match(text, /cpd-context/);
});

test("cpd-manager starts Plans with a brief, messages Leads and never answers for the User", () => {
  const named = commandsNamed(skillFiles("cpd-manager")[0]!.text);
  const has = (re: RegExp) => named.some((c) => re.test(c));
  assert.ok(has(/^cpd plan new .*--context .*--title .*--brief-file/), "plan new with --context, --title and --brief-file");
  assert.ok(has(/^cpd session send /), "session send");
  for (const kind of ["decide", "note", "ask", "answer"]) assert.ok(has(new RegExp(`^cpd context ${kind} `)), `context ${kind}`);
  assert.ok(has(/^cpd plan list/) && has(/^cpd plan show/), "plan list and plan show");
  const text = skillFiles("cpd-manager")[0]!.text;
  assert.match(text, /Never answer for the User/);
  assert.match(text, /never moves the chat slot/);
});

/** The text of a markdown section: from its heading line to the next heading of the same or a higher level. */
function section(text: string, heading: string): string {
  const level = /^#+/.exec(heading)![0].length;
  const lines = text.split("\n");
  const start = lines.indexOf(heading);
  assert.ok(start >= 0, `missing heading: ${heading}`);
  const end = lines.findIndex((l, i) => i > start && new RegExp(`^#{1,${level}} `).test(l));
  return lines.slice(start, end < 0 ? undefined : end).join("\n");
}

test("cpd-lead is organised by the CPD flow: Context, Plan and Deploy with their steps, in order", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  const headings = text.split("\n").filter((l) => /^#{2,3} /.test(l));
  const flow = headings.slice(headings.indexOf("## 1. Context"));
  assert.deepEqual(flow, [
    "## 1. Context", "### Grounding", "### Clarifying", "### Specifying",
    "## 2. Plan", "### Dividing", "### Sequencing", "### Approving", "### Dispatching",
    "## 3. Deploy", "### Mechanical checks", "### PR", "### Reviewing and Monitoring", "### Addressing",
  ]);
  assert.match(section(text, "### Specifying"), /cpd plan phase plan/);
  assert.match(section(text, "### Dispatching"), /\| `job-merged` \|/, "the Events table stays under Dispatching");
});

test("cpd-lead: no local review before pr open, and the overlaps are settled before the Plan review", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  const deploy = section(text, "## 3. Deploy");
  assert.match(deploy, /No local review before `pr open`/);
  assert.match(deploy, /final review Job stays optional/);
  const approving = section(text, "### Approving");
  assert.ok(approving.indexOf("## Overlaps") >= 0, "Approving reads the ## Overlaps section of plan render");
  assert.ok(approving.indexOf("## Overlaps") < approving.indexOf("Call `cpd_plan_review`"), "overlaps are settled before the review is called");
  assert.match(approving, /cpd context note/);
});

test("the skill adds later batches as drafts and approves each with cpd_plan_review in any phase", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  assert.match(text, /--pr <key> --draft/, "later batches are added with job add --draft");
  assert.match(text, /### Starting a later batch[\s\S]*cpd_plan_review[\s\S]*### Appending fixes/, "a later batch starts with a Plan review");
  assert.doesNotMatch(text, /no Plan review for appended work/, "the append-in-chat flow is gone");
  assert.ok(synopses().get("job add")!.has("--draft"), "commands.md documents job add --draft");
});

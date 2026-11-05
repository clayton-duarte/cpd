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
    // The command is the longest prefix of its words that is a synopsis key: a literal positional after the
    // command words (`cpd` in `cpd session send cpd "<text>"`) is an argument, not a word of the command.
    const cmdWords = words(command.slice("cpd ".length));
    let key = cmdWords.join(" ");
    for (let n = cmdWords.length; n > 0; n--) {
      const prefix = cmdWords.slice(0, n).join(" ");
      if (known.has(prefix)) { key = prefix; break; }
    }
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
    // The object is a Workflow (pro-d136): the skill text may say `workflow`, but the cpd-1 forms the word replaced
    // (its command group, its flag and the phrases the glossary retires) stay forbidden.
    assert.doesNotMatch(text, /cpd workflow (init|show|phase)\b|workflow_required|workflow worktree|sync Job|dispatch or pr|phase pr\b|pr\.gate/, `${name}: a retired form of the object is left`);
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
  assert.match(text, /cpd session send <context> "<text>" --unblocks/);
  assert.match(text, /session-message/);
  assert.match(text, /cpd context show --since/);
  assert.doesNotMatch(text, /cpd-context/, "Leads get no turn-start cpd-context message (hs-d209)");
});

test("cpd-manager starts Plans with a brief, messages Leads and never answers for the User", () => {
  const named = commandsNamed(skillFiles("cpd-manager")[0]!.text);
  const has = (re: RegExp) => named.some((c) => re.test(c));
  assert.ok(has(/^cpd plan new .*--context .*--title .*--brief-file/), "plan new with --context, --title and --brief-file");
  assert.ok(has(/^cpd session send /), "session send");
  for (const kind of ["decide", "note"]) assert.ok(has(new RegExp(`^cpd context ${kind} `)), `context ${kind}`);
  assert.ok(has(/^cpd plan list/) && has(/^cpd plan show/), "plan list and plan show");
  const text = skillFiles("cpd-manager")[0]!.text;
  assert.match(text, /you never answer a Lead's question in the User's place|Answer a Lead only with what the User said/);
  assert.match(text, /never moves the chat slot/);
  assert.match(text, /--unblocks/);
  const keep = text.split("\n").find((l) => l.startsWith("- **Keep the work moving.**"));
  assert.ok(keep, "the keep-moving rule exists");
  assert.ok(keep.includes("--unblocks"));
});

test("cpd-manager does not watch a Worker's Escalation: the digest and the stall check name none, and it still never answers one", () => {
  const text = skillFiles("cpd-manager")[0]!.text;
  const line = (start: string) => text.split("\n").find((l) => l.startsWith(start)) ?? assert.fail(`no line ${start}`);
  assert.doesNotMatch(line("- **Keep the work moving.**"), /Escalation/i);
  assert.doesNotMatch(line("- **The turn-start message.**"), /Escalation/i);
  assert.match(line("- A Plan's phases are"), /`job-failed`/);
  assert.doesNotMatch(line("- A Plan's phases are"), /escalation/i);
  assert.match(text, /never a Worker's Escalation/);
});

test("cpd-manager may create Leads and open sessions, and states its focus", () => {
  const text = skillFiles("cpd-manager")[0]!.text;
  const named = commandsNamed(text);
  for (const cmd of ["cpd plan new", "cpd session open", "cpd session fork"]) assert.ok(named.some((c) => c.startsWith(cmd)), `${cmd} is named`);
  const forbidden = text.split("\n").find((l) => l.startsWith("- ") && l.includes("`cpd plan decide`") && l.includes("`cpd context manager`"));
  assert.ok(forbidden, "the forbidden list exists");
  assert.doesNotMatch(forbidden, /cpd session open|cpd session fork/);
  const allowed = text.split("\n").find((l) => l.startsWith("- You may create Leads and open sessions"));
  assert.ok(allowed, "the allowed rule exists");
  assert.match(allowed, /cpd plan new/);
  assert.match(allowed, /cpd session open/);
  assert.match(allowed, /cpd session fork/);
  assert.match(text, /coordinate the Leads, own the Epic and the Context, make sure the work keeps moving, and help the User turn requirements into actionable Plans/);
  assert.match(text, /Leads? .*escalates? directly to the User/);
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

/** The Lead skill's heading of the first step of the Dispatch stage (it holds the Events table). */
const DISPATCH_STEPS = "### Documenting, Red testing, Implementing, Verifying";

test("cpd-lead Escalations: ask the User directly, never through the Manager", () => {
  const esc = section(skillFiles("cpd-lead")[0]!.text, "#### Escalations");
  assert.match(esc, /ask the User directly/);
  assert.match(esc, /never through the Manager/);
});

test("cpd-lead is organised by the CPD flow: Context, Plan and Dispatch with their steps, in order", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  const headings = text.split("\n").filter((l) => /^#{2,3} /.test(l));
  const flow = headings.slice(headings.indexOf("## 1. Context"));
  assert.deepEqual(flow, [
    "## 1. Context", "### Grounding", "### Grilling",
    "## 2. Plan", "### Clarifying", "### Scoping", "### Splitting", "### Orchestrating",
    "## 3. Dispatch", DISPATCH_STEPS, "### Deploying",
  ]);
  const deploying = section(text, "### Deploying").split("\n").filter((l) => /^#### /.test(l));
  assert.deepEqual(deploying, [
    "#### Mechanical checks", "#### PR", "#### Pull request review", "#### Posting for review", "#### Addressing", "#### Review comments",
  ]);
  assert.match(section(text, "### Grilling"), /cpd plan phase plan/);
  assert.match(section(text, DISPATCH_STEPS), /\| `job-merged` \|/, "the Events table stays under the first Dispatch step");
});

/** The three stages and eleven steps of the CPD flow, in order (CONTEXT.md "The CPD flow"; contract §3.9). */
const STAGES = ["Context", "Plan", "Dispatch"];
const STEPS = [
  "Grounding", "Grilling", "Clarifying", "Scoping", "Splitting", "Orchestrating",
  "Documenting", "Red testing", "Implementing", "Verifying", "Deploying",
];

test("the Lead skill and CONTEXT.md name the same three stages and eleven steps, in order", () => {
  const flow = skillFiles("cpd-lead")[0]!.text.split("\n").filter((l) => /^#{2,3} /.test(l));
  const inFlow = flow.slice(flow.indexOf("## 1. Context"));
  assert.deepEqual(inFlow.filter((l) => l.startsWith("## ")).map((l) => l.replace(/^## \d+\. /, "")), STAGES, "the skill's stage headings");
  assert.deepEqual(inFlow.filter((l) => l.startsWith("### ")).flatMap((l) => l.slice("### ".length).split(", ")), STEPS, "the skill's step headings");
  const glossary = readFileSync(new URL("../../CONTEXT.md", here), "utf8");
  const entries = (kind: string) => [...glossary.matchAll(new RegExp(`^\\*\\*([^*]+)\\*\\* \\(${kind}\\b`, "gm"))].map((m) => m[1]!);
  assert.deepEqual(entries("stage").map((n) => n.replace(/ stage$/, "")), STAGES, "CONTEXT.md's stage entries");
  assert.deepEqual(entries("step").filter((n) => STEPS.includes(n)), STEPS, "CONTEXT.md's step entries (Addressing is a twelfth, inside Deploying)");
});

test("cpd-lead: no local review before the PR opens, and the overlaps are settled before the Plan review", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  const deploy = section(text, "### Deploying");
  assert.match(deploy, /No local review before the PR opens/);
  const approving = section(text, "### Orchestrating");
  assert.ok(approving.indexOf("## Overlaps") >= 0, "Orchestrating reads the ## Overlaps section of plan render");
  assert.ok(approving.indexOf("## Overlaps") < approving.indexOf("Call `cpd_plan_review`"), "overlaps are settled before the review is called");
  assert.match(approving, /cpd context note/);
});

test("the skill adds a scoped waiting Job queued with --needs and approves undecided batches with cpd_plan_review in any phase", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  assert.match(text, /--workflow <key> --needs <k1,k2>\n/, "a scoped Job that waits is added queued with its needs link");
  assert.doesNotMatch(text, /--needs <k1,k2> --draft/, "a waiting Job is never held as a draft (hs-d822)");
  assert.doesNotMatch(text, /--draft[^\n`]*--needs|--needs[^\n`]*--draft/, "no command holds a draft with a needs link");
  assert.doesNotMatch(text, /waits on an explore Job|whose `--needs` name an explore Job/, "the explore-waiting form is not a legal draft form (hs-n677)");
  assert.match(text, /### Starting a later batch[\s\S]*cpd_plan_review[\s\S]*### Appending fixes/, "a later batch starts with a Plan review");
  assert.doesNotMatch(text, /no Plan review for appended work/, "the append-in-chat flow is gone");
  assert.ok(synopses().get("job add")!.has("--draft"), "commands.md documents job add --draft");
});

test("cpd-lead: sizing follows attempt_minutes, dialogs stay short, Specs carry the bounded-command rules", () => {
  const [skillMd, ...refs] = skillFiles("cpd-lead");
  const all = [skillMd!, ...refs];
  for (const { name, text } of all) assert.doesNotMatch(text, /30 min|13 min/, `${name}: the timeout is attempt_minutes`);
  assert.match(skillMd!.text, /attempt_minutes/);
  const questions = all.find((f) => f.name.endsWith("questions.md"))!.text;
  assert.match(questions, /A dialog holds one short question/);
  const spec = all.find((f) => f.name.endsWith("spec-files.md"))!.text;
  for (const rule of ["within 10 minutes", "`timeout`", "synthetic benchmarks", "`cpd events follow`", "`cpd watch`", "`cpd tui`", "`cpd session run`"]) {
    assert.ok(spec.includes(rule), `spec-files.md must state: ${rule}`);
  }
});

test("cpd-lead: a Spec's acceptance names targeted tests and the full suite is the Gate's", () => {
  const [skillMd, ...refs] = skillFiles("cpd-lead");
  const spec = refs.find((f) => f.name.endsWith("spec-files.md"))!.text;
  for (const { name, text } of [skillMd!, { name: "spec-files.md", text: spec }]) {
    assert.match(text, /targeted tests/, `${name}: acceptance names targeted tests`);
    assert.match(text, /full suite is the Gate's/, `${name}: the full suite is the Gate's`);
  }
  assert.doesNotMatch(spec, /npm test -- |test --workspace|`npm test`\s+passes/, "no example acceptance runs a full suite");
});

test("cpd-lead: a PR is docs, red tests by another Worker, implementation; bug fixes reproduce first", () => {
  const [skillMd, ...refs] = skillFiles("cpd-lead");
  const text = skillMd!.text;
  const line = text.split("\n").find((l) => l.startsWith("- **Workflow shape:**"))!;
  assert.ok(line, "Splitting states the Workflow shape");
  const at = (re: RegExp) => { const m = re.exec(line); assert.ok(m, `PR shape must state: ${re}`); return m.index; };
  assert.ok(at(/docs Job/) < at(/red tests/) && at(/red tests/) < at(/implementation Job/) && at(/implementation Job/) < at(/open Job cpd adds/), "order: docs, red tests, implementation, open Job");
  assert.match(line, /different Worker from the implementer, from the docs alone/);
  assert.match(line, /A bug or flake fix starts with a test that reproduces it/);
  assert.match(line, /exact rows/);
  assert.match(line, /docs-only or config-only Workflow skips the test Job/);
  assert.match(text, /cpd job add <tests> .*--needs <docs>\n/);
  assert.match(text, /cpd job add <impl> .*--needs <tests>\n/);
  assert.ok(refs.length > 0);
});

// --- the facts the skills state must be facts of the contract ---
//
// Sources (all read from the repo, so no Rust build is needed): docs/contract.md and docs/commands.md for Event kinds (§5.2 table), error
// codes and their exit numbers (the refusal tables), causes (§3.4), config keys (§8.2), id formats (§2.1), phases (§3.1), hooks (§7.3) and
// flags (the synopses); pi/lib/delivery.ts (`LEAD_MESSAGES`) for the kinds a Lead is sent. `--help` is not used: it needs the binary.

const contractDoc = readFileSync(new URL("../../docs/contract.md", here), "utf8");
const updatesSource = readFileSync(new URL("../lib/updates.ts", here), "utf8");

/** The cells of a markdown table row, or null for any other line. */
function cells(line: string): string[] | null {
  return line.startsWith("|") ? line.split("|").slice(1, -1).map((c) => c.trim()) : null;
}

/** Every backticked token of `text` matching `re`. */
function ticks(text: string, re: RegExp): string[] {
  return [...text.matchAll(/`([^`\n]+)`/g)].map((m) => m[1]!).filter((t) => re.test(t));
}

const SNAKE = /^[a-z]+(_[a-z0-9]+)+$/;
const KEBAB = /^[a-z]+(-[a-z0-9]+)+$/;

/** A cell that is only backticked words (`a`, `b` or `c`): those words. Null for prose. */
function wordsOfCell(cell: string): string[] | null {
  const found = [...cell.matchAll(/`([a-z][a-z_-]*)`/g)].map((m) => m[1]!);
  return found.length > 0 && cell.replace(/`[a-z][a-z_-]*`/g, "").replace(/\b(and|or)\b|[,\s]/g, "") === "" ? found : null;
}

/** contract.md §5.2: kind → its table row. */
function kindRows(): Map<string, string> {
  const rows = new Map<string, string>();
  for (const line of section(contractDoc, "### 5.2 Kinds").split("\n")) {
    const c = cells(line);
    const kinds = c && wordsOfCell(c[0]!);
    if (kinds) for (const k of kinds) rows.set(k, line);
  }
  return rows;
}

/** Every error code of the docs → the exit numbers it is refused with. Rows are `| code | exit | when |` (§1.3) or `| condition | code | exit |` (per command). `error` Event codes have no exit. */
function errorCodes(): Map<string, Set<number>> {
  const codes = new Map<string, Set<number>>();
  const add = (code: string, exit?: number) => {
    const exits = codes.get(code) ?? new Set<number>();
    if (exit !== undefined) exits.add(exit);
    codes.set(code, exits);
  };
  for (const line of [...commandsDoc.split("\n"), ...contractDoc.split("\n")]) {
    const c = cells(line);
    if (!c || c.length < 3) continue;
    const last = c[c.length - 1]!;
    if (/^\d$/.test(last)) for (const w of wordsOfCell(c[c.length - 2]!) ?? []) add(w, Number(last));
    if (/^\d$/.test(c[1]!)) for (const w of wordsOfCell(c[0]!) ?? []) add(w, Number(c[1]));
  }
  for (const m of commandsDoc.matchAll(/`([a-z]+(?:_[a-z]+)+)`[ ,(]+exit (\d)/g)) add(m[1]!, Number(m[2])); // prose: "(`spec_invalid`, exit 2"
  for (const t of ticks(kindRows().get("error")!, SNAKE)) add(t);
  return codes;
}

/** `[tiers]`-style `key = value` lines of the contract's §8.2 TOML block: key → value. */
function configDefaults(): Map<string, string> {
  const map = new Map<string, string>();
  for (const m of section(contractDoc, "### 8.2 Keys").matchAll(/^([a-z_]+)\s*=\s*([^#\n]*?)\s*(?:#.*)?$/gm)) map.set(m[1]!, m[2]!);
  return map;
}

/** Words the skills use that are not error codes: each from the contract table that defines it. */
function vocabulary(): { causes: Set<string>; reasons: Set<string>; configKeys: Set<string>; fields: Set<string>; tools: Set<string> } {
  const fields = new Set<string>();
  for (const line of section(contractDoc, "### 2.2 Objects").split("\n")) for (const w of wordsOfCell(cells(line)?.[0] ?? "") ?? []) fields.add(w);
  // The outline file's fields (commands.md §4.13).
  const outline = section(commandsDoc, "### 4.13 `plan outline`");
  for (const line of outline.split("\n")) for (const w of wordsOfCell(cells(line)?.[0] ?? "") ?? []) fields.add(w);
  const reasons = /reasons` is a subset of `\[([^\]]+)\]`/.exec(kindRows().get("pr-feedback")!)![1]!.match(/[a-z_]+/g)!;
  const contractTools = /--exclude-tools ([a-z_,]+)/.exec(contractDoc)![1]!.split(",");
  // Extract pi tool names from commands.md headings: "The pi tool `name`"
  const piTools: string[] = [];
  for (const match of commandsDoc.matchAll(/^### \d+\.\d+ The pi tool `([a-z_]+)`/gm)) {
    piTools.push(match[1]!);
  }
  const tools = [...contractTools, ...piTools];
  return {
    causes: new Set(ticks(section(contractDoc, "### 3.4 Attempt statuses, outcomes and causes"), /^[a-z]+(_[a-z]+)*$/)),
    reasons: new Set(reasons),
    configKeys: new Set(configDefaults().keys()),
    fields,
    tools: new Set(tools),
  };
}

/** Every flag of a `cpd` synopsis of commands.md (any command). */
function knownFlags(): Set<string> {
  return new Set([...synopses().values()].flatMap((f) => [...f]));
}

const planIdPattern = () => /\| Plan id \| (`[^`]+`)/.exec(section(contractDoc, "### 2.1 Identifiers"))![1]!.slice(1, -1);
const idFormat = (thing: string) => new RegExp(`^\\| ${thing} \\| \`([^\`]+)\``, "m").exec(section(contractDoc, "### 2.1 Identifiers"))![1]!;

/** Hooks, Event kinds and ids that look like kinds: `pre-*` hooks are defined in §7.3; `cpd-context` is the adapter's message type; `cpd-policy` its system prompt section (contract §8.3); `pay-4521` is the Plan id example. */
const notKinds = (hooks: Set<string>) => new Set([...hooks, "cpd-context", "cpd-policy", "pay-4521"]);

/** Every fact `files` state that the docs do not: an empty list means every code, kind, flag, number and id exists. */
function factProblems(files: { name: string; text: string }[]): string[] {
  const problems: string[] = [];
  const kinds = kindRows();
  const codes = errorCodes();
  const vocab = vocabulary();
  const flags = knownFlags();
  const hooks = new Set(["pre-start", "pre-commit", "pre-merge"].filter((h) => section(contractDoc, "### 7.3 Hooks from the repo policy file").includes(`| \`${h}\` |`)));
  const defaults = configDefaults();
  const edges = new Map<string, string>(); // the first transition out of each phase
  for (const m of section(contractDoc, "### 3.1 Plan phases").matchAll(/^\s+([a-z]+) --> ([a-z]+):/gm)) if (!edges.has(m[1]!)) edges.set(m[1]!, m[2]!);
  const phasePath = ["grounding"]; // the forward path of the state diagram: grounding → plan → dispatch → deploy → done
  while (edges.has(phasePath.at(-1)!) && phasePath.length < 10 && !phasePath.includes(edges.get(phasePath.at(-1)!)!)) phasePath.push(edges.get(phasePath.at(-1)!)!);
  const recordLetters = /letter `d` decision, `n` note, `p` plan, `b` brief/.test(section(contractDoc, "### 2.1 Identifiers")) ? "dnpb" : "";
  assert.ok(recordLetters, "§2.1 record id letters not parsed");
  assert.ok(kinds.size > 40 && codes.size > 60 && hooks.size === 3 && flags.size > 40, `docs not parsed: ${kinds.size} kinds, ${codes.size} codes, ${hooks.size} hooks, ${flags.size} flags`);

  for (const { name, text } of files) {
    const bad = (what: string) => problems.push(`${name}: ${what}`);
    // Event kinds: every backticked kebab-case word that is not a hook or a known id is a kind of contract §5.2.
    for (const t of ticks(text, KEBAB)) if (!notKinds(hooks).has(t) && !kinds.has(t)) bad(`\`${t}\` is not an Event kind (contract.md §5.2) nor a hook`);
    // Error codes: every backticked snake_case word is an error code, a cause, a pr-feedback reason, a config key, a field or a tool, each from its own table.
    for (const t of ticks(text, SNAKE)) {
      if (!codes.has(t) && !vocab.causes.has(t) && !vocab.reasons.has(t) && !vocab.configKeys.has(t) && !vocab.fields.has(t) && !vocab.tools.has(t)) bad(`\`${t}\` is no error code, cause, reason, config key, field or tool of the docs`);
    }
    // In the positions that say "error code", the word must be an error code, with the exit number the skill states.
    for (const m of text.matchAll(/(?:\bcode|refused with|refused:) `([a-z_]+)`/g)) if (!codes.has(m[1]!)) bad(`"${m[0]}": \`${m[1]}\` is not an error code`);
    for (const m of text.matchAll(/`([a-z_]+)` \(exit (\d)\)/g)) {
      const exits = codes.get(m[1]!);
      if (!exits) bad(`"${m[0]}": \`${m[1]}\` is not an error code`);
      else if (!exits.has(Number(m[2]))) bad(`"${m[0]}": commands.md gives \`${m[1]}\` exit ${[...exits].join(" or ")}`);
    }
    // Flags: every --flag the text names, in a command or in prose, is a flag of some cpd synopsis (a `gh`, `git`, `scripts/cargo.sh`, `node` or `npm` command is not cpd's).
    for (const f of new Set(text.replace(/`(?:gh|git|timeout \d+ node|scripts\/cargo\.sh|node|npm) [^`\n]*`/g, "").match(/(?<![\w-])--[a-z][a-z-]*/g))) if (!flags.has(f)) bad(`${f} is no flag of any cpd command (commands.md)`);
    // `error.details.x`: a field of the details of some refusal (not the `details` of pi's ask_user_question result).
    for (const m of text.matchAll(/\berror\.details\.([a-z_]+)/g)) if (!commandsDoc.includes(`details.${m[1]}`)) bad(`error.details.${m[1]} is in no commands.md refusal row`);
    // Ids: record ids are <machine>-<letter><n>; the skill's patterns are the contract's.
    for (const m of text.matchAll(/\bhs-([a-z])(\d+)\b/g)) if (!recordLetters.includes(m[1]!) || Number(m[2]) < 1) bad(`${m[0]} is not a Context record id (contract.md §2.1)`);
    for (const m of text.matchAll(/`(\^[^`]+\$)`/g)) {
      const formats = ["Context id", "Plan id", "Workflow key \\(R34\\)", "Job key"].map(idFormat);
      if (!formats.includes(m[1]!)) bad(`id pattern \`${m[1]}\` is none of contract.md §2.1`);
    }
    for (const [re, format, thing] of [
      [/\b[Ii]d `(\^[^`]+\$)`/, planIdPattern(), "Plan id"],
      [/\b(?:PR|Workflow) has a key \(`(\^[^`]+\$)`/, idFormat("Workflow key \\(R34\\)"), "Workflow key"],
      [/id matches `(\^[^`]+\$)`/, planIdPattern(), "Plan id"],
      [/Job key matches `(\^[^`]+\$)`/, idFormat("Job key"), "Job key"],
    ] as const) {
      for (const m of text.matchAll(new RegExp(re, "g"))) if (m[1] !== format) bad(`the ${thing} pattern is \`${m[1]}\`, contract.md §2.1 says \`${format}\``);
    }
    if (/<attempt-id>-q<k>/.test(text) && !/`<attempt>-q<k>`/.test(section(contractDoc, "### 2.1 Identifiers"))) bad("the Escalation id format is not `<attempt>-q<k>` (contract.md §2.1)");
    // Phases: a chain that starts at `grounding` is the forward path of contract.md §3.1.
    for (const m of text.matchAll(/\bgrounding(?: → [a-z]+)+/g)) if (m[0] !== phasePath.join(" → ")) bad(`"${m[0]}" is not the Plan phases of contract.md §3.1 (${phasePath.join(" → ")})`);
    // Numbers: what the skill states about config defaults is the contract's default.
    const stated = (re: RegExp, key: string) => {
      for (const m of text.matchAll(new RegExp(re, "g"))) if (Number(m[1]) !== Number(defaults.get(key))) bad(`"${m[0]}": contract.md §8.2 sets ${key} = ${defaults.get(key)}`);
    };
    stated(/`retry_cap`, default (\d+),/, "retry_cap");
    stated(/`ask_minutes`, default (\d+) min/, "ask_minutes");
    stated(/every (\d+) s\b/, "pr_poll_seconds");
    for (const m of text.matchAll(/so (\d+) Attempts\)/g)) if (Number(m[1]) !== 1 + Number(defaults.get("retry_cap"))) bad(`"${m[0]}": automatic Attempts are 1 + retry_cap (contract.md §9)`);
    for (const m of text.matchAll(/last (\d+) lines/g)) if (!commandsDoc.includes(`\`details.tail\` is its last ${m[1]} lines`)) bad(`"${m[0]}": commands.md says \`details.tail\` is its last 60 lines`);
    for (const m of text.matchAll(/(?:At most|up to) (\d+) (?:lines|Jobs)/g)) {
      const mgr = section(commandsDoc, "### 15.7 Context updates at turn start");
      if (!mgr.includes(`At most ${m[1]} lines`) && !mgr.includes(`up to ${m[1]} lines`)) bad(`"${m[0]}": commands.md §15.7 does not say ${m[1]}`);
    }
  }
  return problems;
}

test("`cpd-context`, the turn-start message the Manager skill names, is the adapter's message type", () => {
  assert.match(updatesSource, /CONTEXT_TYPE = "cpd-context"/, "`cpd-context` is the adapter's message type");
});

for (const { skill } of skills) test(`${skill}: every code, Event kind, flag, number and id it states exists in the docs`, () => {
  assert.deepEqual(factProblems(skillFiles(skill)), []);
});

test("the fact check fails on a code, a kind, a flag, an exit number, a number, an id and a phase that do not exist", () => {
  const good = skillFiles("cpd-lead")[0]!;
  assert.deepEqual(factProblems([good]), []);
  const problems = (line: string) => factProblems([{ name: "bad.md", text: `${good.text}\n${line}\n` }]).join("\n");
  for (const [line, culprit] of [
    ["The Gate ends with `gate_busted` (exit 3).", /gate_busted/], // an unknown error code
    ["It is refused with `no_such_code`.", /no_such_code/], // an unknown code where a code is named
    ["The Lead gets `job-exploded` messages.", /job-exploded/], // an unknown Event kind
    ["Run `cpd job retry <k> --turbo`.", /--turbo/], // an unknown flag
    ["Add `--turbo-mode` to go faster.", /--turbo-mode/], // a flag in prose, which the commands check does not see
    ["`gate_failed` (exit 1)", /gate_failed.*exit 3/], // a wrong exit number
    ["The engine polls every 7 s.", /pr_poll_seconds = 5/], // a wrong poll interval
    ["The cap is `retry_cap`, default 5, retries.", /retry_cap = 2/], // a wrong default
    ["Escalations expire after `ask_minutes`, default 15 min.", /ask_minutes = 60/], // a wrong default
    ["See hs-x9.", /hs-x9/], // a record id with an unknown letter
    ["A Plan has an id `^[a-z]+$`.", /\^\[a-z\]\+\$/], // an id pattern of no kind
    ["The key is `^[a-z][a-z0-9-]{1,23}$`; each Workflow has a key (`^[a-z][a-z0-9-]{1,23}$`, for example `web`).", /Workflow key pattern/], // the Plan pattern for a Workflow key
    ["Phases: grounding → plan → dispatch → pr → done.", /grounding → plan → dispatch → pr → done/], // a renamed phase
    ["Read `error.details.nothing`.", /error\.details\.nothing/], // a detail no refusal carries
  ] as const) assert.match(problems(line), culprit, line);
});

test("every Event kind of the Lead's Events table is one the adapter delivers to a Lead, and each kind that wakes the Lead has a row", async () => {
  const { LEAD_MESSAGES } = await import("../lib/delivery.ts");
  const dispatching = section(skillFiles("cpd-lead")[0]!.text, DISPATCH_STEPS);
  const table = dispatching.slice(0, dispatching.indexOf("#### "));
  const rowKinds = table.split("\n").flatMap((l) => /^\| `([a-z-]+)` \|/.exec(l)?.slice(1) ?? []);
  assert.ok(rowKinds.length >= 6, `expected the Events table, found ${rowKinds.length} rows`);
  for (const k of rowKinds) assert.ok(k in LEAD_MESSAGES, `${k}: a row of the Events table, but the adapter delivers no such message to a Lead (pi/lib/delivery.ts LEAD_MESSAGES)`);
  for (const [k, { turn }] of Object.entries(LEAD_MESSAGES)) if (turn) assert.ok(rowKinds.includes(k), `${k} starts a Lead's turn but the Events table has no row for it`);
  const rows = kindRows();
  for (const line of table.split("\n")) {
    const kind = /^\| `([a-z-]+)` \|/.exec(line)?.[1];
    if (!kind) continue;
    for (const m of line.matchAll(/`data\.([a-z_]+)`/g)) assert.match(rows.get(kind)!, new RegExp(`\\b${m[1]}\\b`), `${kind}: data.${m[1]} is not in its contract.md §5.2 row`);
  }
});

test("the skills' list of pr-feedback reasons and of ladder causes is the contract's", () => {
  const reasons = vocabulary().reasons;
  assert.ok(reasons.size === 6); // reasons: checks_failed, changes_requested, comments, closed, ready_to_post, approved (pro-d64; pro-d130 removed the post-readiness reason, pro-d225 restored it)
  const lead = skillFiles("cpd-lead")[0]!.text;
  const stated = /`data\.reasons` lists ((?:`[a-z_]+`(?:, | or )?)+)/.exec(lead);
  assert.ok(stated, "the pr-feedback row lists data.reasons");
  assert.deepEqual(new Set(wordsOfCell(stated[1]!)), reasons);
  const ladder = /Ladder causes\*\* are ((?:`[a-z_]+`(?:, | and )?)+)/.exec(contractDoc);
  assert.ok(ladder, "contract.md names the ladder causes");
  const spec = skillFiles("cpd-lead").find((f) => f.name.endsWith("spec-files.md"))!.text;
  const retried = /ladder retries \(((?:`[a-z_]+`(?:, )?)+)\)/.exec(spec);
  assert.ok(retried, "spec-files.md lists the causes the ladder retries");
  assert.deepEqual(wordsOfCell(retried[1]!), wordsOfCell(ladder[1]!));
  const failed = section(lead, "#### Failed Jobs").split("\n").flatMap((l) => (/^\| (`[^|]+`)(?: \([^)]*\))? \|/.exec(l)?.[1] ? [/^\| (`[^|]+`)/.exec(l)![1]!] : []));
  assert.ok(failed.length >= 7, "the Failed Jobs table lists the causes");
  for (const cause of failed.flatMap((c) => ticks(c, /./))) assert.ok(vocabulary().causes.has(cause), `Failed Jobs: ${cause} is no cause of contract.md §3.4`);
});

test("cpd-lead registers planned work at once; cpd-manager checks it", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  assert.match(lead, /Register planned work at once/);
  assert.match(lead, /--needs <k1,k2>/, "a scoped Job that waits is queued with its needs link (hs-d822)");
  for (const token of ["under review", "queued", "draft_parked", "draft_incomplete"]) assert.ok(lead.includes(token), `the two legal draft forms: ${token}`);
  assert.match(lead, /cpd workflow add <key> --repo <path> --title "<title>" --needs/);
  assert.match(lead, /workflow_title_required/);
  assert.match(lead, /cpd workflow edit <key> --title/);
  for (const l of lead.split("\n")) if (/cpd workflow add <key>/.test(l)) assert.match(l, /--title/, `workflow add without --title: ${l}`);
  assert.doesNotMatch(lead, /cpd_plan_review` \(no parameters\)/);
  // Workflow needs are stated at workflow add with --needs <workflow-key> or --no-needs, refused needs_required in dispatch
  assert.match(lead, /--needs <workflow-key>/);
  assert.match(lead, /--no-needs/);
  assert.match(lead, /needs_required/);
  const [skillMd, ...refs] = skillFiles("cpd-lead");
  const spec = refs.find((f) => f.name.endsWith("spec-files.md"))!.text;
  assert.match(spec, /--needs <research-workflow>/);
  const manager = skillFiles("cpd-manager")[0]!.text;
  assert.match(manager, /Check each Plan's registered work/);
  assert.match(manager, /cpd plan show --plan <p>/);
  assert.match(manager, /cpd session send <context>:<plan>[^\n]*register/);
});

test("cpd-manager splits work by area, maps files and sets the landing order", () => {
  const manager = skillFiles("cpd-manager")[0]!.text;
  assert.match(manager, /Split by deliverable and area/);
  assert.match(manager, /cpd context pending --context <c>/);
  assert.match(manager, /Set the landing order/);
});

test("both skills describe a stacked PR: gh pr edit --base, then cpd pr push", () => {
  for (const skill of ["cpd-lead", "cpd-manager"]) {
    const text = skillFiles(skill)[0]!.text;
    assert.match(text, /gh pr edit <n> --base/, `${skill}: gh pr edit --base`);
    assert.match(text, /cpd workflow add <key> --repo <path> --base/, `${skill}: workflow add --base`);
    assert.match(text, /cpd pr push --workflow <key>/, `${skill}: cpd pr push`);
  }
});

test("cpd-lead: a stacked PR waits for its base PR to open, --base needs neither --needs nor --no-needs, and a draft is no parking place", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  assert.match(text, /waiting.*base Workflow is open/, "cpd-lead: stacked Workflow waits for base Workflow to open");
  assert.match(text, /needs neither flag/, "cpd-lead: --base needs neither flag");
  assert.match(text, /draft.*never.*parking place/, "cpd-lead: draft is no parking place");
});

test("cpd-lead: a same-repo PR on an earlier PR of the Plan is stacked with --base <pr-key> by default, in each of its five places", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  const bullet = (start: string) => text.split("\n").find((l) => l.startsWith(start)) ?? assert.fail(`no line ${start}`);
  // 1. Rules: Register planned work at once
  const register = section(text, "## Rules");
  assert.match(register, /same-repo follow-up is stacked \(`--base <workflow-key>`, the default\)/);
  assert.match(register, /`--needs <workflow-key>` \(the cross-repo or fresh-main exception: `cpd workflow add <key> --repo <path> --title "<title>" --needs <workflow-key>`\)/);
  // 2. 2. Plan, Scoping: the settling of repos and Workflows
  assert.match(section(text, "### Scoping"), /stacked on it with `--base <workflow-key>`, the default; `--needs <workflow-key>` only for another repo or a Workflow that must start from fresh `main`/);
  // 3. 2. Plan, Scoping: the stacked example, the order between Workflows, the base Workflow added before
  const scoping = section(text, "### Scoping");
  assert.match(scoping, /cpd workflow add <key> --repo <path> --title "<title>" --base <workflow-key>\n/);
  assert.match(scoping, /\*\*The order between Workflows\.\*\* A same-repo Workflow that depends on an earlier Workflow of the Plan is stacked on it: `--base <workflow-key>`, never `--needs`/);
  assert.match(scoping, /starts as soon as its base Workflow is `open`[^\n]*without waiting for the base Workflow's review or merge/);
  assert.match(scoping, /a Workflow may need, or stack on, only Workflows added before it/);
  // 4. Splitting (the old Sequencing): the Dependencies bullet and the Workflow needs bullet (which names the post-lock restack)
  assert.match(bullet("- A Job's Dependencies are Jobs of its own Workflow"), /stacking, `--base <workflow-key>`, the default for a same-repo Workflow/);
  const needs = bullet("- **Workflow needs before the lock:**");
  assert.match(needs, /`cpd workflow edit <key> --base <workflow-key> --no-needs`/, "the post-lock restack edit is named");
  assert.match(needs, /`workflow_locked` refuses every other `--base`, `--needs` or `--repo` edit after the lock/);
  // 5. 3. Dispatch, Deploying, Stacked Workflows
  const registerLayer = bullet("- Register a later layer at once");
  assert.match(registerLayer, /`--base <workflow-key>`[^\n]*the default/);
  assert.match(registerLayer, /`--needs <workflow-key>` is the exception, for another repo or a Workflow that must start from fresh `main`/);
  assert.doesNotMatch(registerLayer, /Use `--needs <workflow-key>` instead/, "--needs is no longer the way to wait for a same-repo base Workflow");
  assert.match(bullet("- While the previous Workflow is open"), /after the lock, `cpd workflow edit` changes a waiting Workflow's base only in the restack of \[Splitting\]\(#splitting\)/);
});

test("cpd-lead references: the research follow-up keeps --needs <research-pr> as the fresh-main exception to stacking", () => {
  const spec = skillFiles("cpd-lead").find((f) => f.name.endsWith("spec-files.md"))!.text;
  const line = spec.split("\n").find((l) => l.includes("--needs <research-workflow>")) ?? assert.fail("no research follow-up sentence");
  assert.match(line, /the exception to stacking \(\[Scoping\]\(\.\.\/SKILL\.md#scoping\)\)/);
});

test("cpd-manager: the landing-order line stacks a same-repo Workflow and keeps --needs for other repos, not for the Workflows of a Plan", () => {
  const line = skillFiles("cpd-manager")[0]!.text.split("\n").find((l) => l.startsWith("- **Set the landing order.**")) ?? assert.fail("no landing-order line");
  assert.match(line, /the Lead stacks a same-repo Workflow on the one it depends on \(`cpd workflow add <key> --repo <path> --base <workflow-key>`\), `--needs` across repos/);
  assert.doesNotMatch(line, /needs order|order its own Workflows|each starts when the other merges/, "needs no longer orders the Workflows of a Plan");
});

test("urgent Jobs: both skills state the flag, the command and the explicit-request rule", () => {
  for (const skill of ["cpd-lead", "cpd-manager"]) {
    const text = skillFiles(skill)[0]!.text;
    assert.match(text, /cpd job urgent <k>/, `${skill}: cpd job urgent`);
    assert.match(text, /--urgent/, `${skill}: --urgent`);
    assert.match(text, /only when the User explicitly asks/, `${skill}: explicit-request rule`);
    assert.match(text, /beyond `concurrency`/, `${skill}: may exceed concurrency`);
  }
  assert.ok(synopses().get("job urgent")!.has("--plan"), "commands.md documents job urgent");
});

/** Contract §8.3 "Delivery to agents": the shared skills and the adapter carry cpd mechanics only. No owner's choice and no Context record id. */
test("policy-neutral: no record id, owner or machine fact in the skills, pi/lib or the extension", () => {
  const root = new URL("../../", import.meta.url);
  const files: { name: string; text: string }[] = [];
  for (const skill of skills) for (const f of skillFiles(skill.skill)) files.push({ name: `pi/skills/${f.name}`, text: f.text });
  for (const f of readdirSync(new URL("lib/", new URL("../", here))).sort()) {
    if (f.endsWith(".ts") && !f.endsWith(".test.ts")) files.push({ name: `pi/lib/${f}`, text: readFileSync(new URL(`pi/lib/${f}`, root), "utf8") });
  }
  files.push({ name: "pi/extension.ts", text: readFileSync(new URL("pi/extension.ts", root), "utf8") });
  assert.ok(files.length > 10, "the guard must find the files it guards");
  const forbidden = [/\bhs-[dnqap][0-9]+/, /clayton-duarte/, /missionlane/, /homeserver/, /User's machines/];
  const hits: string[] = [];
  for (const { name, text } of files) {
    // pi/lib/secret.ts names the vaults `cpd` and `homeserver` that cpd secret opens without a grant (commands.md §15.17): a cpd mechanic, not an owner's choice.
    const skip = name === "pi/lib/secret.ts" ? ["homeserver"] : [];
    text.split("\n").forEach((line, i) => {
      for (const re of forbidden) if (!skip.includes(re.source) && re.test(line)) hits.push(`${name}:${i + 1}: ${re} in: ${line.trim().slice(0, 100)}`);
    });
  }
  assert.deepEqual(hits, []);
});

/** core/src/policy/default.toml names the keys `gate.merge` (the quick check) and `gate.pr` (the Gate); the Gate runs only when the PR's `gate.pr` is `local`. */
test("cpd-lead: every 'run(s) the Gate' sentence names gate.pr", () => {
  const sentences = skillFiles("cpd-lead")[0]!.text.split(/(?<=[.;!?])\s+/).filter((s) => /\bruns? the Gate\b/i.test(s));
  // The feedback-fix rewrite (pro-d68) left one such sentence in the skill: the minimum is that count, and each must still name gate.pr.
  assert.ok(sentences.length >= 1, "the guard must find the sentences it guards");
  const bad = sentences.filter((s) => !s.includes("gate.pr"));
  assert.deepEqual(bad, []);
});

test("cpd-lead adds decided work queued and keeps drafts for undecided design (hs-d301)", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  assert.match(text, /added queued/);
  assert.match(text, /Spec that cites the decision/);
  assert.match(text, /When unsure, a draft/);
});

test("cpd-manager names the decision of routed decided work and says it needs no review (hs-d301)", () => {
  const text = skillFiles("cpd-manager")[0]!.text;
  assert.match(text, /name the decision id/);
  assert.match(text, /needs no Plan review/);
});

for (const { skill } of skills) test(`${skill}: asks the User only with ask_user_question, never a Context record (hs-d306, pro-d8)`, () => {
  const text = skillFiles(skill)[0]!.text;
  assert.match(text, /To ask the User, use `ask_user_question`/);
  assert.match(text, /file outside the repo and point to it in chat/);
  assert.doesNotMatch(text, /cpd context (ask|answer)\b/, "context ask and context answer are removed");
});

test("cpd-lead leaves a PR's conflict to the update Job and a feedback fix is one work Job pushed by its own merge; the hand repair pushes through the Gate, never a plain git push or an update Job added by hand (hs-d305, hs-d340, pro-d68)", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  assert.ok(!/(?<!never a |Never push with a )plain `git push`/.test(text));
  assert.ok(!/run no local tests/i.test(text));
  assert.ok(!/hs-d220/.test(text));
  const row = text.split("\n").find((l) => l.startsWith("| `pr-feedback`"))!;
  const addressing = section(text, "#### Addressing");
  const rule = text.split("\n").find((l) => /`git` and `gh` are allowed/.test(l))!;
  assert.match(row, /A conflict with the base is no reason and sends you nothing/);
  assert.match(row, /Append one Job per actionable item to the Workflow named in `data\.workflow` with `cpd job add/);
  assert.match(row, /The fix Job's own merge pushes it to the pull request: nothing else is queued/);
  assert.match(row, /cpd queues an update Job for it/);
  assert.doesNotMatch(row, /cpd queue the PR's update Job/);
  assert.doesNotMatch(row, /cpd pr (rebase|push)/, "a feedback fix is pushed by its own merge: the row sends the Lead to no push command");
  assert.doesNotMatch(row, /`conflict`/, "a conflict is no pr-feedback reason (hs-d340)");
  assert.match(addressing, /Append ONE `work` Job per cohesive fix/);
  assert.match(addressing, /The Job's merge pushes the Lead Branch to the pull request: nothing else is queued, no Gate runs/);
  assert.match(addressing, /A conflict with the base is unchanged: cpd queues an `update` Job \(never for a base that merely moved\)/);
  assert.doesNotMatch(addressing, /Queuing the first fix Job makes cpd queue/);
  assert.match(addressing, /never a plain `git push`/);
  assert.match(addressing, /never by adding an update Job yourself/);
  assert.match(addressing, /No `--kind`, no test, impl or docs Job, no `--draft`, no review: the Job is queued at once/);
  assert.match(addressing, /`cpd pr rebase --workflow <key>`/);
  assert.match(addressing, /`cpd pr push --workflow <key>`/);
  assert.match(rule, /`cpd pr push` or `cpd pr rebase`/);
  assert.match(rule, /never with `git push`/);
  assert.match(rule, /the Workflow's open and update Jobs push it/);
});

test("cpd-manager never blocks and says the adapter refuses waits over 30 seconds", () => {
  const text = skillFiles("cpd-manager")[0]!.text;
  assert.match(text, /30 seconds/);
  assert.match(text, /background/);
});

test("cpd-lead never blocks and says the adapter refuses waits over 30 seconds", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  assert.match(text, /\*\*Stay free for the User\.\*\*/);
  assert.match(text, /30 seconds/);
});

test("cpd opens the PRs: cpd-lead never opens one or waits, cpd-manager oversees the PRs whose Jobs are all merged (hs-d340)", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  assert.match(lead, /\*\*cpd opens the pull requests, you do not\.\*\*/);
  assert.match(lead, /never wait for anything to open a pull request/);
  assert.match(lead, /`cpd pr open` is the repair/);
  assert.doesNotMatch(lead, /all-merged/, "no Event `all-merged` is appended any more (contract §3.7 E4)");
  assert.doesNotMatch(lead, /Open a PR as soon as its last Job merges/);
  const manager = skillFiles("cpd-manager")[0]!.text;
  assert.match(manager, /Oversee all-merged Workflows/);
  assert.match(manager, /stack those that would conflict/);
  assert.match(manager, /cpd opens the pull request as soon as a Workflow's last Job merges/);
});

test("the Job kinds: the template and its shape, [tests], the kind of each Job and the red-test guard (hs-d341, hs-d497)", () => {
  const [skillMd, ...refs] = skillFiles("cpd-lead");
  const lead = skillMd!.text;
  const dividing = section(lead, "### Scoping");
  assert.match(dividing, /cpd workflow add <key> --repo <path> --title "<title>" --shape docs/);
  assert.match(dividing, /cpd workflow add <key> --repo <path> --title "<title>" --shape config/);
  assert.match(dividing, /`<key>-docs` \(kind docs\), `<key>-tests` \(kind test, needs the docs Job\) and `<key>-impl` \(kind impl, needs the test Job\)/);
  assert.match(dividing, /stored and enforced/);
  assert.match(dividing, /`shape_violation`/);
  assert.match(dividing, /`shape_incomplete`/);
  assert.match(dividing, /`tests_unconfigured`/);
  assert.match(dividing, /you add it yourself, as a commit in the Lead worktree of a Workflow of that repo/);
  const splitting = section(lead, "### Splitting");
  assert.match(splitting, /`--kind docs\|test\|impl`/);
  assert.match(splitting, /a test Job's also has `tests` and `test_files`/);
  const approving = section(lead, "### Orchestrating");
  assert.match(approving, /the kind of each Job and, for each Workflow that has not started, the placeholder row of its open Job/);
  const escalations = section(lead, "#### Escalations");
  assert.match(escalations, /`cpd job approve-tests <job> --path <p> --reason "<why>"`/);
  assert.match(escalations, /only an implementation Job/);
  const spec = refs.find((f) => f.name.endsWith("spec-files.md"))!.text;
  const tests = section(spec, "## Test Jobs");
  for (const word of ["`tests`", "`test_files`", "`[test-commands]`", "`build`", "`run`", "`list`", "`tests_not_found`", "guarded", "cpd job approve-tests", "derived"]) assert.ok(tests.includes(word), `spec-files.md Test Jobs must state ${word}`);
  const fields = /with exactly these fields.*$/m.exec(spec)![0];
  assert.match(fields, /a test Job's also has `tests` and `test_files`/);
});

test("the open Job and the update Job: cpd adds them, the Lead decides only when one fails (hs-d340, hs-d490, hs-d497)", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  const checks = section(lead, "#### Mechanical checks");
  assert.match(checks, /The checks are part of the Workflow's open Job \(key `open-<workflow>`/);
  assert.match(checks, /You run none of them/);
  assert.match(checks, /only when the Lead Branch conflicts with it/);
  assert.match(checks, /a base that merely moved ahead is no reason/);
  assert.match(checks, /`gate_failed` when the Gate/);
  assert.match(checks, /you append no fix Jobs for a failed Gate/);
  assert.match(checks, /ready and never a draft/);
  assert.match(checks, /`open_job_live`/);
  const rows = section(lead, DISPATCH_STEPS);
  const merged = rows.split("\n").find((l) => l.startsWith("| `job-merged`"))!;
  assert.match(merged, /`data\.number` and `data\.url`/);
  assert.match(merged, /Do not open a pull request yourself/);
  assert.match(merged, /A fix Job of an open Workflow is done when its `job-merged` arrives: its merge pushed the Lead Branch to the pull request/);
  assert.doesNotMatch(merged, /cpd queues an update Job that pushes them/);
  const failed = section(lead, "#### Failed Jobs");
  assert.match(failed, /A Workflow's open or update Job/);
  const pr = section(lead, "#### PR");
  assert.match(pr, /the Job's `job-merged` Event carries the URL \(`data\.url`\)/);
  assert.match(pr, /if the pull request then conflicts with it, cpd queues an update Job; you do nothing/);
  const fixes = section(lead, "#### Appending fixes");
  assert.match(fixes, /A Gate failure of a Workflow's open or update Job is not one/);
  assert.match(fixes, /On a Workflow that is already open, a fix Job is a `work` Job and its own merge carries it to the pull request/);
  assert.doesNotMatch(fixes, /makes cpd queue the PR's update Job/);
});

test("cpd-manager: job approve-tests joins the commands that belong to a Lead and pr open leaves them (hs-d340, hs-d341)", () => {
  const manager = skillFiles("cpd-manager")[0]!.text;
  const rule = manager.split("\n").find((l) => l.startsWith("- ") && l.includes("`cpd plan phase plan`") && l.includes("`cpd job approve-tests`"))!;
  assert.match(rule, /`cpd job approve-tests`/);
  assert.doesNotMatch(rule, /`cpd pr open`/);
});

test("the Worker Spec's text and the adapter's refusal say the full suite is the Gate's, run by the Workflow's open Job (hs-d340)", () => {
  const worker = readFileSync(new URL("../../core/src/spec/worker.md", here), "utf8");
  // The object is a Workflow (pro-d136, pro-d138): the Spec and the adapter's code both speak the new word; the rename is a cutover, no transitional period.
  assert.match(worker, /the Gate runs it, in the Workflow's open Job/);
  const guard = readFileSync(new URL("../lib/testguard.ts", here), "utf8");
  assert.match(guard, /the full suite is the Gate's, run by the Workflow's open Job/);
  for (const text of [worker, guard]) assert.doesNotMatch(text, /at pr open/);
  const deliveries = readFileSync(new URL("../lib/delivery.ts", here), "utf8");
  assert.doesNotMatch(deliveries, /"all-merged":/, "commands.md §15.3: all-merged is ignored");
});

test("a PR is a substantial deliverable: cpd-lead gets parallelism from Jobs and folds small PRs; cpd-manager splits by deliverable and area", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  assert.match(lead, /A Workflow is a substantial deliverable\./);
  assert.match(lead, /parallelism from Jobs inside a Workflow, not from more Workflows/);
  assert.match(lead, /one small Job joins the Workflow of its area/);
  assert.match(lead, /combine finished Workflows into one before they open/);
  const manager = skillFiles("cpd-manager")[0]!.text;
  assert.match(manager, /Split by deliverable and area/);
  assert.match(manager, /small follow-up goes into its area's existing Workflow, not a new one/);
});

test("both skills: decisions are ask rounds at any phase, G2 limit stays and G4 guard enforced, larger content goes to a file", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  const manager = skillFiles("cpd-manager")[0]!.text;
  for (const text of [lead, manager]) {
    assert.match(text, /every open decision goes to the User as an ask round/);
    assert.match(text, /enforces G2.*1500-character limit/);
    assert.match(text, /and G4/);
    assert.doesNotMatch(text, /G3/);
    assert.match(text, /State what you ask about in the question itself/);
    assert.doesNotMatch(text, /First write what you ask about in the chat/);
    assert.doesNotMatch(text, /refuses `ask_user_question` for a Lead whose Plan has draft/);
  }
  const approving = section(lead, "### Orchestrating");
  assert.match(approving, /cpd plan outline --file/);
  assert.match(approving, /`Approve` and `Change`/);
  assert.match(approving, /outline_missing[\s\S]*outline_incomplete/);
});

test("cpd-lead merges the base only on a conflict; ready is its own Gate and no conflict", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  assert.match(lead, /only when the Lead Branch conflicts with it/);
  assert.match(lead, /its Gate passed on its own head and GitHub reports no conflict/);
  assert.match(lead, /Its open Job checks exactly that/);
  // `base_behind` is retired (hs-d490): cpd refuses only a conflict, `base_conflict`.
  assert.doesNotMatch(lead, /base_behind/);
  assert.doesNotMatch(lead, /contains (the|its) base|if it moved/i);
});

test("cpd-manager pins the exact CLI of context decide, plan new, session send and job retry", () => {
  const rules = section(skillFiles("cpd-manager")[0]!.text, "## Rules");
  const lines = rules.split("\n");
  const head = lines.indexOf("- **Exact CLI syntax (never guess flags or subcommands):**");
  assert.ok(head >= 0, "the Exact CLI syntax bullet");
  const subs: string[] = [];
  for (const l of lines.slice(head + 1)) {
    if (!/^  - /.test(l)) break;
    subs.push(l);
  }
  const expected = [
    { words: "context decide", flags: ["--context"] },
    { words: "plan new", flags: ["--context", "--title", "--brief-file"] },
    { words: "session send", flags: ["--unblocks"] },
    { words: "job retry", flags: ["--plan", "--tier"] },
  ];
  assert.equal(subs.length, expected.length, "exactly four sub-bullets");
  const syn = synopses();
  expected.forEach((e, i) => {
    const span = /^  - `(cpd [^`]+)`/.exec(subs[i]!)?.[1];
    assert.ok(span?.startsWith(`cpd ${e.words} `), `bullet ${i + 1} starts with cpd ${e.words}`);
    const flags = [...span!.matchAll(/--[a-z][a-z-]*/g)].map((m) => m[0]);
    for (const f of e.flags) assert.ok(flags.includes(f), `${e.words} carries ${f}`);
    const allowed = syn.get(e.words);
    assert.ok(allowed, `synopsis of ${e.words}`);
    for (const f of flags) assert.ok(allowed.has(f), `${f} is a flag of ${e.words}`);
  });
  assert.ok(subs[0]!.includes("positional") && subs[0]!.includes("NO `--title` or `--body`"));
  assert.ok(subs[2]!.includes("always required") && subs[2]!.includes("broadcast"));
  assert.ok(subs[3]!.includes("always required"));
});

test("cpd-manager and cpd-lead carry the pro-d204 architecture rule and the four-step bug report protocol", () => {
  const cases = [
    { skill: "cpd-manager", scope: "Plan / Workflow / Job" },
    { skill: "cpd-lead", scope: "Workflow / Job" },
  ];
  for (const c of cases) {
    const lines = section(skillFiles(c.skill)[0]!.text, "## Rules").split("\n");
    const arch = lines.find((l) => l.startsWith("- **Skills are for agent discovery; "));
    assert.ok(arch, `${c.skill}: architecture bullet`);
    assert.ok(arch.includes("Skills teach the agent what exists and when to use it"), `${c.skill}: teach`);
    assert.ok(arch.includes("never ad-hoc prose"), `${c.skill}: never ad-hoc prose`);
    const at = lines.findIndex((l) => l.startsWith("- **Bug report protocol"));
    assert.ok(at >= 0, `${c.skill}: protocol bullet`);
    const texts = [
      "Debug, research and identify both the immediate mitigation and the long-term root-cause fix",
      "Apply the mitigation ASAP",
      `Queue the follow-up root-cause fix (${c.scope})`,
      "Include regression tests so the bug can never be reintroduced",
    ];
    texts.forEach((t, i) => {
      const line = lines[at + 1 + i] ?? "";
      assert.ok(line.startsWith(`  ${i + 1}. `), `${c.skill}: step ${i + 1} numbered`);
      assert.ok(line.includes(t), `${c.skill}: step ${i + 1} text`);
    });
    assert.ok(!/^ {2}5\. /.test(lines[at + 5] ?? ""), `${c.skill}: no fifth step`);
  }
});

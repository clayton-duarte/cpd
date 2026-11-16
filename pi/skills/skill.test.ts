import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";

const here = new URL("./", import.meta.url);
const commandsDoc = readFileSync(new URL("../../docs/commands.md", here), "utf8");

/** Role skills whose stated commands and facts must agree with the documentation. */
const skills = [
  { skill: "cpd-lead" },
  { skill: "cpd-manager" },
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
  assert.ok(fields.description!.length > 20, "description identifies the role and its use");
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

for (const { skill } of skills) test(`every cpd command ${skill} names exists in commands.md, with the flags used`, () => {
  const known = synopses();
  const named = skillFiles(skill).flatMap((f) => commandsNamed(f.text).map((c) => ({ file: f.name, command: c })));
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

/** The text of a markdown section: from its heading line to the next heading of the same or a higher level. */
function section(text: string, heading: string): string {
  const level = /^#+/.exec(heading)![0].length;
  const lines = text.split("\n");
  const start = lines.indexOf(heading);
  assert.ok(start >= 0, `missing heading: ${heading}`);
  const end = lines.findIndex((l, i) => i > start && new RegExp(`^#{1,${level}} `).test(l));
  return lines.slice(start, end < 0 ? undefined : end).join("\n");
}

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
  for (const match of commandsDoc.matchAll(/^### \d+\.\d+ .*?[Tt]he pi tool `([a-z_]+)`/gm)) {
    piTools.push(match[1]!);
  }
  // The values of the `kind` parameter of `cpd_external_approve` (commands.md §15.27).
  const kindValues = /`kind` \(([^)]+)\)/.exec(section(commandsDoc, "### 15.27 Guards for raw GitHub comment writes and Slack sends, and the pi tool `cpd_external_approve`"))?.[1]?.match(/[a-z]+_[a-z]+/g) ?? [];
  const tools = [...contractTools, ...piTools, ...kindValues];
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

test("`cpd-context` is the adapter's turn-start message type", () => {
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

// The retired assertions prescribed phases, command choreography, and adapter mechanics.
// These contracts instead cover role outcomes and authority; runtime guard coverage remains separate.
test("Manager preserves product decisions and delegates bounded outcomes", () => {
  const text = skillFiles("cpd-manager")[0]!.text;
  for (const requirement of [
    /ask the User when intent, priority, or authority is genuinely unclear/,
    /Record settled decisions and durable context in the appropriate shared record/,
    /Keep unresolved questions open until the User settles them/,
    /Give a Lead the goal, relevant context and decision records, authorized scope, acceptance evidence, and exclusions/,
    /Let authorized executors investigate implementation facts and choose implementation details within the agreed outcome/,
    /bring the concrete options and consequences to the User, preserve the answer, and return it to the Lead/,
  ]) assert.match(text, requirement);
});

test("Lead shapes and delegates a delivery contract without prescribing implementation", () => {
  const text = skillFiles("cpd-lead")[0]!.text;
  for (const requirement of [
    /Define the goal, relevant context, authorized scope, checkable acceptance evidence, and exclusions/,
    /approval applies to the authorized outcome and risk, not clerical serialization/,
    /Give each Worker the goal, the facts and decisions they need, the authority and scope they have, acceptance evidence, and exclusions/,
    /A Worker may investigate facts within the authorized scope and need not follow a precomputed implementation/,
    /escalate when intent or permission is missing/,
    /Coordinate actual dependencies before they block execution/,
  ]) assert.match(text, requirement);
});

test("roles split work for substantive reasons, not ceremony", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  const manager = skillFiles("cpd-manager")[0]!.text;
  assert.match(lead, /Split only for demonstrated independence or parallel benefit, independent verification, distinct permissions or deliverables, or work too large/);
  assert.match(manager, /Split only when independent ownership, meaningful parallel benefit, distinct permissions or deliverables, or the size of the work justifies it/);
});

test("roles preserve authority, secrets, external safeguards and runtime controls", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  const manager = skillFiles("cpd-manager")[0]!.text;
  assert.match(lead, /Protect secrets and leave consequential external actions under the human control and safeguards already in force; role guidance grants no new authority/);
  assert.match(manager, /Respect the User's authority, the authorized scope, secret protections, and the controls for consequential external actions/);
  assert.match(manager, /Execution preferences do not grant new permissions/);
  for (const text of [lead, manager]) {
    assert.match(text, /(?:does not|do not) replace or disable runtime controls/);
    assert.match(text, /(?:Never bypass a guard|bypass a control to force success)/);
  }
  assert.match(lead, /current Worker runtime is not thereby guaranteed to discover or load that skill/);
});

test("roles report evidence and blockers honestly", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  const manager = skillFiles("cpd-manager")[0]!.text;
  assert.match(lead, /Match verification to the deliverable and risk/);
  assert.match(lead, /A passing test proves only what it tested/);
  assert.match(lead, /report the blocker, its evidence, and the intervention needed/);
  assert.match(lead, /Report what changed, what was checked and its result, what remains, and any limitation/);
  assert.match(manager, /Distinguish work completed from verification, review, merge, and deployment/);
  assert.match(manager, /Do not claim completion without evidence/);
  assert.match(manager, /state what is blocked and what decision or intervention is needed/);
});


test("external-write authority distinguishes Lead bot replies from Manager authority", () => {
  const lead = skillFiles("cpd-lead")[0]!.text;
  const manager = skillFiles("cpd-manager")[0]!.text;
  for (const text of [lead, manager]) {
    assert.match(text, /Human PR comments and reviews, actions on other PRs, and Slack posts require explicit User permission and applicable approval controls/);
    assert.match(text, /\[external-write approval boundary\]\(\.\.\/\.\.\/\.\.\/docs\/commands\.md#1527-guards-for-raw-github-comment-writes-and-slack-sends-and-the-pi-tool-cpd_external_approve\)/);
  }
  assert.match(lead, /Only replies to bot review threads on your own Workflow PR may proceed without renewed permission, through permitted tools and applicable controls/);
  assert.doesNotMatch(manager, /bot review threads|without renewed permission/);
  assert.match(commandsDoc, /^### 15\.27 Guards for raw GitHub comment writes and Slack sends, and the pi tool `cpd_external_approve`$/m);
});

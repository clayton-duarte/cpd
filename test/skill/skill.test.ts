import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { MESSAGE_KINDS } from "../../src/queue/messages.ts";
import { runCli } from "../helpers/index.ts";

const root = new URL("../../", import.meta.url);
const skillDir = new URL("pi/skills/cpd-lead/", root);
const read = (url: URL) => readFileSync(url, "utf8");

/** SKILL.md plus every file under references/: the whole text the Lead can load. */
function skillFiles(): { name: string; text: string }[] {
  const refs = existsSync(new URL("references/", skillDir)) ? readdirSync(new URL("references/", skillDir)).sort() : [];
  return [
    { name: "SKILL.md", text: read(new URL("SKILL.md", skillDir)) },
    ...refs.map((f) => ({ name: `references/${f}`, text: read(new URL(`references/${f}`, skillDir)) })),
  ];
}

// --- frontmatter ---

/** The Agent Skills frontmatter subset the skill uses: `key: value` lines between two `---` fences. */
function frontmatter(text: string): { fields: Record<string, string>; body: string } {
  const m = /^---\n([\s\S]*?)\n---\n([\s\S]*)$/.exec(text);
  assert.ok(m, "SKILL.md must start with a --- frontmatter block");
  const fields: Record<string, string> = {};
  for (const line of m[1]!.split("\n")) {
    const kv = /^([a-z][a-z-]*): (.+)$/.exec(line);
    assert.ok(kv, `frontmatter line is not "key: value": ${line}`);
    assert.ok(!(kv[1]! in fields), `duplicate frontmatter key: ${kv[1]}`);
    fields[kv[1]!] = kv[2]!.replace(/^(["'])(.*)\1$/, "$2");
  }
  return { fields, body: m[2]! };
}

test("frontmatter is valid: name cpd-lead, a description that triggers on a CPD Workflow", () => {
  const { fields } = frontmatter(read(new URL("SKILL.md", skillDir)));
  assert.deepEqual(Object.keys(fields).sort(), ["description", "name"]);
  assert.equal(fields.name, "cpd-lead");
  assert.match(fields.name!, /^[a-z0-9]+(-[a-z0-9]+)*$/);
  assert.ok(fields.description!.length <= 1024, "pi allows descriptions up to 1024 characters");
  assert.match(fields.description!, /\bUse when\b/);
  assert.match(fields.description!, /starts or continues a CPD Workflow/);
});

// --- command lines (contract §11) ---

/** Flags per command, from contract §11. `--wf` selects the Workflow where needed. */
const FLAGS: Record<string, string[]> = {
  doctor: [],
  tiers: [],
  "lead start": [],
  "lead attach": ["wf"],
  "workflow init": ["wf", "repo", "title", "base"],
  "workflow show": ["wf"],
  "workflow phase": ["wf"],
  "job add": ["wf", "key", "title", "tier", "spec", "needs", "review-tier"],
  "job edit": ["wf", "title", "tier", "spec", "needs", "review-tier"],
  "job list": ["wf", "status"],
  "job show": ["wf"],
  "job cancel": ["wf", "reason"],
  "job retry": ["wf", "tier"],
  "job dispatch": ["wf"],
  "plan render": ["wf", "out"],
  "plan decision": ["wf", "review-id", "pending", "approved", "rejected", "feedback-file"],
  "plan lock": ["wf", "no-watch"],
  watch: ["wf"],
  "attempt run": ["wf", "slot"],
  "attempt submit": ["wf"],
  "attempt read": ["wf", "lines"],
  "attempt list": ["wf"],
  "attempt show": ["wf"],
  "review decide": ["wf", "approve", "reject", "feedback"],
  "session rename": ["session"],
  ask: ["wf", "timeout-minutes"],
  answer: ["wf"],
  "pr open": ["wf"],
  "pr poll": ["wf"],
  "pr rebase": ["wf"],
};
const COMMANDS_WITHOUT_WF = new Set(["doctor", "tiers", "lead start", "session rename"]);
const SWITCHES = new Set(["approve", "reject", "pending", "approved", "rejected", "no-watch"]);
/** Positional arguments per command (contract §11); absent means none. */
const POSITIONALS: Record<string, number> = {
  "job edit": 1, "job show": 1, "job cancel": 1, "job retry": 1, "job dispatch": 1,
  "attempt run": 1, "attempt submit": 1, "attempt read": 1, "attempt show": 1,
  "review decide": 1, "workflow phase": 1, "session rename": 1, ask: 2, answer: 2,
};
/** Flags the contract marks as required. */
const REQUIRED: Record<string, string[]> = {
  "workflow init": ["wf", "repo", "title"],
  "job add": ["key", "title", "tier", "spec"],
  "job cancel": ["reason"],
  "job retry": ["tier"],
  "review decide": ["feedback"],
};
/** `[a, b]` means one of a or b. */
const ONE_OF: Record<string, string[]> = { "review decide": ["approve", "reject"] };

/** `cpd` command lines in code spans and fenced blocks. Prose outside code is not a command. */
function commandLines(text: string): { line: number; text: string }[] {
  const found: { line: number; text: string }[] = [];
  let fenced = false;
  text.split("\n").forEach((raw, i) => {
    if (/^\s*```/.test(raw)) {
      fenced = !fenced;
      return;
    }
    const candidates = fenced ? [raw.trim().replace(/^\$ /, "")] : [...raw.matchAll(/`([^`]+)`/g)].map((m) => m[1]!.trim());
    for (const c of candidates) if (/^cpd( |$)/.test(c) && c !== "cpd") found.push({ line: i + 1, text: c });
  });
  return found;
}

/** Problems with one command line; an empty list means it names a real command with contract flags. */
function problems(line: string, commands: ReadonlySet<string>): string[] {
  const tokens = line.match(/"[^"]*"|'[^']*'|\S+/g)!.slice(1);
  const two = `${tokens[0]} ${tokens[1]}`;
  const name = commands.has(two) ? two : tokens[0]!;
  if (!commands.has(name)) return [`unknown command: ${name}`];
  const rest = tokens.slice(name.split(" ").length);
  const flags: string[] = [];
  let positionals = 0;
  const out: string[] = [];
  for (let i = 0; i < rest.length; i++) {
    const t = rest[i]!;
    if (!t.startsWith("--")) {
      positionals++;
      continue;
    }
    const flag = t.slice(2);
    if (!FLAGS[name]!.includes(flag)) out.push(`flag --${flag} is not in contract §11 for \`cpd ${name}\``);
    flags.push(flag);
    if (!SWITCHES.has(flag)) i++; // its value
  }
  if (!COMMANDS_WITHOUT_WF.has(name) && !flags.includes("wf")) out.push("missing --wf <wf>");
  for (const f of REQUIRED[name] ?? []) if (!flags.includes(f)) out.push(`missing required --${f}`);
  const oneOf = ONE_OF[name];
  if (oneOf && !oneOf.some((f) => flags.includes(f))) out.push(`needs one of ${oneOf.map((f) => `--${f}`).join(" or ")}`);
  if (positionals !== (POSITIONALS[name] ?? 0)) out.push(`expected ${POSITIONALS[name] ?? 0} positional argument(s), found ${positionals}`);
  return out;
}

async function cliCommands(): Promise<Set<string>> {
  const r = await runCli(["--help"]);
  assert.equal(r.code, 0, r.stdout + r.stderr);
  return new Set<string>(r.json.commands);
}

test("the flag allowlist covers exactly the commands in src/cli.ts's table", async () => {
  assert.deepEqual([...(await cliCommands())].sort(), Object.keys(FLAGS).sort());
});

test("the command-line checker rejects what it must (so a pass means something)", async () => {
  const commands = await cliCommands();
  const ok = 'cpd job add --key api --title "API" --tier S1 --spec /tmp/api.json --needs a,b --wf pay-4521';
  assert.deepEqual(problems(ok, commands), []);
  assert.deepEqual(problems('cpd answer api-a1-q1 "use sqlite" --wf pay-4521', commands), []);
  assert.deepEqual(problems("cpd review decide api-a1 --approve --feedback ok --wf w", commands), []);
  assert.deepEqual(problems("cpd tiers", commands), []);
  assert.match(problems("cpd job nope --wf w", commands).join(), /unknown command/);
  assert.match(problems(`${ok} --bogus x`, commands).join(), /--bogus is not in contract/);
  assert.match(problems(ok.replace(" --wf pay-4521", ""), commands).join(), /missing --wf/);
  assert.match(problems("cpd job add --key a --wf w", commands).join(), /missing required --title/);
  assert.match(problems("cpd job retry --tier S3 --wf w", commands).join(), /positional/);
  assert.match(problems("cpd review decide a --feedback x --wf w", commands).join(), /one of --approve or --reject/);
  assert.match(problems("cpd tiers --wf w", commands).join(), /--wf is not in contract/);
});

test("every cpd command line in the skill names a real command with contract flags", async () => {
  const commands = await cliCommands();
  const bad: string[] = [];
  let seen = 0;
  for (const f of skillFiles()) {
    for (const c of commandLines(f.text)) {
      seen++;
      for (const p of problems(c.text, commands)) bad.push(`${f.name}:${c.line}: ${p}: ${c.text}`);
    }
  }
  assert.ok(seen >= 20, `expected many command lines, found ${seen}`);
  assert.deepEqual(bad, []);
});

test("the skill never runs herdr, bd, wt, gh or git (naming them to forbid them is fine)", () => {
  const bad: string[] = [];
  for (const f of skillFiles()) {
    let fenced = false;
    f.text.split("\n").forEach((raw, i) => {
      if (/^\s*```/.test(raw)) fenced = !fenced;
      const candidates = fenced ? [raw.trim()] : [...raw.matchAll(/`([^`]+)`/g)].map((m) => m[1]!.trim());
      for (const c of candidates) if (/^(herdr|bd|wt|gh|git) \S/.test(c)) bad.push(`${f.name}:${i + 1}: ${c}`);
    });
  }
  assert.deepEqual(bad, []);
});

test("every protocol command of Context, Plan, Dispatch and PR appears", () => {
  const lines = skillFiles().flatMap((f) => commandLines(f.text).map((c) => c.text));
  for (const want of [
    "cpd workflow init", "cpd lead attach", "cpd workflow phase", "cpd job add", "cpd job edit", "cpd plan render",
    "cpd plan lock", "cpd job retry", "cpd job cancel", "cpd answer", "cpd review decide", "cpd pr open",
    "cpd pr rebase", "cpd workflow show", "cpd attempt read",
  ]) {
    assert.ok(lines.some((l) => l.startsWith(`${want} `)), `no command line for ${want}`);
  }
  const text = skillFiles().map((f) => f.text).join("\n");
  assert.match(text, /`ask_user_question`/);
  assert.match(text, /`cpd_plan_review`/);
});

// --- queue messages (contract §10) ---

/** Message kinds from the contract's §10 table. */
function contractKinds(): string[] {
  const doc = read(new URL("docs/cli-contract.md", root));
  const section = /\n## 10\.[\s\S]*?\n## 11\./.exec(doc);
  assert.ok(section, "contract §10 not found");
  return [...section[0].matchAll(/^\| `([a-z-]+)` \|/gm)].map((m) => m[1]!);
}

test("§10 message kinds in the contract match the code", () => {
  assert.deepEqual([...contractKinds()].sort(), [...MESSAGE_KINDS].sort());
});

/** The dispatch table's rows: first cell a kind in code, last cell the action. */
function dispatchRows(): Map<string, string> {
  const rows = new Map<string, string>();
  for (const line of read(new URL("SKILL.md", skillDir)).split("\n")) {
    const m = /^\| `([a-z-]+)` \|(.*)\|\s*$/.exec(line);
    if (m) rows.set(m[1]!, m[2]!.split("|").pop()!.trim());
  }
  return rows;
}

test("every §10 message kind has a row with an action in the dispatch table", () => {
  const rows = dispatchRows();
  for (const kind of contractKinds()) {
    assert.ok(rows.has(kind), `no dispatch row for ${kind}`);
    assert.ok(rows.get(kind)!.length > 0, `empty action for ${kind}`);
  }
});

test("the rows name the actions the requirements assign to each kind", () => {
  const rows = dispatchRows();
  const wants: Record<string, string[]> = {
    "job-failed": ["cpd job retry", "cpd job cancel", "cpd job add", "ask_user_question"],
    escalation: ["cpd answer", "ask_user_question"],
    "review-needed": ["cpd review decide"],
    "all-merged": ["cpd pr open"],
    "pr-feedback": ["cpd job add", "cpd pr rebase"],
  };
  for (const [kind, needles] of Object.entries(wants)) {
    for (const n of needles) assert.ok(rows.get(kind)?.includes(n), `row ${kind} does not mention ${n}`);
  }
});

// --- rules the Lead must be given ---

test("the skill states the sizing rule and the Tier examples", () => {
  const text = skillFiles().map((f) => f.text).join("\n");
  assert.match(text, /13 min/);
  assert.match(text, /150 turns/);
  assert.match(text, /30 min/);
  for (const t of ["S1", "S4", "H2"]) assert.ok(text.includes(`\`${t}\``), `Tier example ${t} missing`);
});

test("references are linked from SKILL.md and exist", () => {
  const skill = read(new URL("SKILL.md", skillDir));
  const linked = [...skill.matchAll(/\]\((references\/[^)#]+\.md)(?:#[^)]*)?\)/g)].map((m) => m[1]!);
  const present = skillFiles().map((f) => f.name).filter((n) => n.startsWith("references/"));
  assert.deepEqual([...new Set(linked)].sort(), present.sort());
});

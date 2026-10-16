import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";

const here = new URL("./", import.meta.url);
const skillDir = new URL("cpd-lead/", here);
const commandsDoc = readFileSync(new URL("../../docs/commands.md", here), "utf8");

/** SKILL.md plus every file under references/: the whole text the Lead can load. */
function skillFiles(): { name: string; text: string }[] {
  const refs = readdirSync(new URL("references/", skillDir)).sort();
  return [
    { name: "SKILL.md", text: readFileSync(new URL("SKILL.md", skillDir), "utf8") },
    ...refs.map((f) => ({ name: `references/${f}`, text: readFileSync(new URL(`references/${f}`, skillDir), "utf8") })),
  ];
}

test("frontmatter: name cpd-lead and a description that says when to use it", () => {
  const m = /^---\n([\s\S]*?)\n---\n/.exec(skillFiles()[0]!.text);
  assert.ok(m, "SKILL.md must start with a --- frontmatter block");
  const fields: Record<string, string> = {};
  for (const line of m[1]!.split("\n")) {
    const kv = /^([a-z][a-z-]*): (.+)$/.exec(line);
    assert.ok(kv, `frontmatter line is not "key: value": ${line}`);
    assert.ok(!(kv[1]! in fields), `duplicate frontmatter key: ${kv[1]}`);
    fields[kv[1]!] = kv[2]!;
  }
  assert.deepEqual(Object.keys(fields).sort(), ["description", "name"]);
  assert.equal(fields.name, "cpd-lead");
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

/** Every `cpd …` command the text names: inline code spans and lines of fenced blocks (not the `cpd adapter:` message prefix). */
function commandsNamed(text: string): string[] {
  const found: string[] = [];
  const fenced = text.replace(/```[^\n]*\n([\s\S]*?)```/g, (_all, body: string) => {
    for (const line of body.split("\n")) if (line.startsWith("cpd ")) found.push(line.trim());
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

test("every cpd command the skill names exists in commands.md, with the flags used", () => {
  const known = synopses();
  const named = skillFiles().flatMap((f) => commandsNamed(f.text).map((c) => ({ file: f.name, command: c })));
  assert.ok(named.length > 40, `expected many commands, found ${named.length}`);
  for (const { file, command } of named) {
    const key = words(command.slice("cpd ".length)).join(" ");
    const flags = known.get(key);
    assert.ok(flags, `${file}: "${command}": "cpd ${key}" is not a command of commands.md`);
    for (const f of command.matchAll(/--[a-z][a-z-]*/g)) {
      assert.ok(flags.has(f[0]), `${file}: "${command}": ${f[0]} is not a flag of "cpd ${key}"`);
    }
  }
});

test("nothing of cpd 1 or of the removed Reviewer is left", () => {
  for (const { name, text } of skillFiles()) {
    assert.doesNotMatch(text, /--wf\b|\bCPD_WF\b/, `${name}: --wf is cpd 1`);
    assert.doesNotMatch(text, /\bworkflows?\b/i, `${name}: the term Workflow is retired`);
    assert.doesNotMatch(text, /herdr|\bbd\b|queue message|reviewMode|review-tier|review-needed|review decide|outOfScope/i, `${name}: cpd 1 or removed Reviewer text`);
  }
});

test("relative links resolve to files of the skill", () => {
  const files = new Set(skillFiles().map((f) => f.name));
  for (const { name, text } of skillFiles()) {
    for (const link of text.matchAll(/\]\((references\/[^)#]+)(?:#[^)]*)?\)/g)) {
      assert.ok(files.has(link[1]!), `${name}: broken link ${link[1]}`);
    }
  }
});

test("rules allow git and gh for diagnosis, still forbid tmux, wt, force pushes and product commits", () => {
  const text = skillFiles()[0]!.text;
  const rule = text.split("\n").find((l) => l.startsWith("- `git` and `gh`") || /`git` and `gh` are allowed/.test(l));
  assert.ok(rule, "Rules must say what git and gh may do");
  assert.match(rule, /Never run `tmux` or `wt`/);
  assert.match(rule, /never force-push/);
  assert.match(rule, /never commit product changes/);
  assert.ok(!/Never run `git`/.test(text), "git is no longer forbidden");
  assert.match(text, /gh pr checks <n>[^|\n]*gh run view <run-id> --log-failed/);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";

const skills = ["cpd-lead", "cpd-manager"] as const;

/** SKILL.md plus every file under references/ (if any). */
function skillFiles(skill: string): { name: string; text: string }[] {
  const skillDir = new URL(`../skills/${skill}/`, import.meta.url);
  const refsDir = new URL("references/", skillDir);
  const refs = existsSync(refsDir) ? readdirSync(refsDir).sort() : [];
  return [
    { name: `${skill}/SKILL.md`, text: readFileSync(new URL("SKILL.md", skillDir), "utf8") },
    ...refs.map((f) => ({
      name: `${skill}/references/${f}`,
      text: readFileSync(new URL(`references/${f}`, skillDir), "utf8"),
    })),
  ];
}

function skillText(skill: string): string {
  return skillFiles(skill)[0]!.text;
}

/** The lines from the heading line to the next heading of the same or a higher level. */
function section(text: string, heading: string): string {
  const lines = text.split("\n");
  const start = lines.findIndex((l) => l === heading);
  assert.ok(start >= 0, `heading ${heading} exists`);
  const level = heading.match(/^#+/)![0].length;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    const m = lines[i]!.match(/^(#+)\s/);
    if (m && m[1]!.length <= level) {
      end = i;
      break;
    }
  }
  return lines.slice(start, end).join("\n");
}

function sendExamples(text: string): { example: string; line: number }[] {
  const out: { example: string; line: number }[] = [];
  text.split("\n").forEach((l, i) => {
    for (const m of l.matchAll(/cpd session send/g)) {
      const rest = l.slice(m.index!);
      const tick = rest.indexOf("`");
      out.push({ example: tick >= 0 ? rest.slice(0, tick) : rest, line: i + 1 });
    }
  });
  return out;
}

test("skill-lateral-comms both skills have an Agent keys section with the hierarchy and the three keys", () => {
  for (const skill of skills) {
    const text = skillText(skill);
    const s = section(text, "## Agent keys");
    for (const lit of [
      "Manager > Lead > Worker",
      "Context > Plan > Job",
      "`<context>`",
      "`<context>:<plan>`",
      "`<context>:<plan>:<job>`",
    ]) {
      assert.ok(s.includes(lit), `${skill}: Agent keys includes ${lit}`);
    }
    assert.match(s, /never[^\n]*`<context>-manager`/, `${skill}: never <context>-manager`);
    assert.ok(
      text.indexOf("## Agent keys") < text.indexOf("## Rules"),
      `${skill}: Agent keys before Rules`,
    );
  }
});

test("skill-lateral-comms neither skill renames a session, names plan new --name or addresses a Manager or Lead by id", () => {
  for (const skill of skills) {
    for (const f of skillFiles(skill)) {
      assert.doesNotMatch(f.text, /session rename/, `${f.name}: session rename`);
      assert.doesNotMatch(f.text, /plan new[^\n]*--name/, `${f.name}: plan new --name`);
      assert.doesNotMatch(f.text, /<manager-id>|<lead-session>/, `${f.name}: id address`);
      assert.doesNotMatch(f.text, /`rename`/, `${f.name}: rename`);
    }
  }
  assert.ok(
    skillText("cpd-lead").includes('cpd session send <context> "<text>" --unblocks "<reason>"'),
    "cpd-lead: send to <context>",
  );
  assert.ok(
    skillText("cpd-manager").includes(
      'cpd session send <context>:<plan> "<text>" --unblocks "<reason>"',
    ),
    "cpd-manager: send to <context>:<plan>",
  );
});

test("skill-lateral-comms both skills send a request to a permanent Manager with session send cpd and --unblocks", () => {
  for (const skill of skills) {
    const s = section(skillText(skill), "### Communication rules");
    for (const lit of [
      "`cpd` for cpd defects",
      "`host` for machine maintenance",
      "`codereview` for code reviews",
      'cpd session send cpd "<text>" --unblocks "cpd defect: <short>"',
      "The `cpd` Manager applies the bug report protocol (pro-d204)",
    ]) {
      assert.ok(s.includes(lit), `${skill}: Communication rules includes ${lit}`);
    }
  }
});

test("skill-lateral-comms Lead-to-Lead is coordination only, Lead-to-Manager escalates and a board post stays FYI only", () => {
  for (const skill of skills) {
    const s = section(skillText(skill), "### Communication rules");
    for (const lit of [
      "Lead-to-Lead (`<context>:<plan>`): coordination only, never assigning work or waiting (pro-d305)",
      "Lead-to-Manager (`<context>`)",
      'cpd session send <context> "<text>" --unblocks "<reason>"',
    ]) {
      assert.ok(s.includes(lit), `${skill}: Communication rules includes ${lit}`);
    }
    assert.match(
      s,
      /cpd board post[^\n]*FYI only, never a request that blocks/,
      `${skill}: board post FYI only`,
    );
  }
});

test("skill-lateral-comms every cpd session send example in both skills carries --unblocks", () => {
  const floors = { "cpd-lead": 4, "cpd-manager": 6 } as const;
  for (const skill of skills) {
    const examples = sendExamples(skillText(skill));
    for (const e of examples) {
      assert.ok(
        e.example.includes("--unblocks"),
        `${skill}: example "${e.example}" at line ${e.line} lacks --unblocks`,
      );
    }
    assert.ok(
      examples.length >= floors[skill],
      `${skill}: ${examples.length} examples, want at least ${floors[skill]}`,
    );
  }
});

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

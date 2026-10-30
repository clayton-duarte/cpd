import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { Fixture, planShow, shown } from "./fake.ts";

/** commands.md §15.23: a Lead or a Manager is told, at its next turn, which of its skill files changed since its baseline. */

type Role = "lead" | "manager" | "worker";
const SHOWN: Record<Role, Record<string, unknown>> = {
  lead: {},
  manager: { role: "manager", plan: null, plan_lead: false },
  worker: { role: "worker", plan_lead: false },
};
const SKILL: Record<Role, string> = { lead: "cpd-lead", manager: "cpd-manager", worker: "" };

const sha = (text: string) => createHash("sha256").update(text).digest("hex");
const dirOf = (f: Fixture, skill: string) => join(f.adapterDir, "skills", skill);
const write = (f: Fixture, skill: string, file: string, text: string) => {
  const path = join(dirOf(f, skill), file);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
};
const remove = (f: Fixture, skill: string, file: string) => rmSync(join(dirOf(f, skill), file), { force: true });
/** The text the fixture writes into each default SKILL.md. */
const DEFAULT = (skill: string) => `the ${skill} skill\n`;

function fixture(t: { after(fn: () => unknown): void }, role: Role = "lead"): Fixture {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown(SHOWN[role]));
  f.load();
  return f;
}

const start = async (f: Fixture, reason = "resume") => {
  await f.pi.fire("session_start", { reason });
  await f.quiet();
};
const turn = async (f: Fixture) =>
  ((await f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "go" })) as any[]).flatMap((r) => (r?.message ? [r.message] : []));
const notices = (ms: any[]) => ms.filter((m) => m.customType === "cpd-skills");
const skillEntries = (f: Fixture) => (f.pi.entries as any[]).filter((e) => e.type === "custom" && e.customType === "cpd-skills");
const skillReports = (f: Fixture, code = "skills_unreadable") => f.reports().filter((r) => r.code === code);
const ending = (dir: string) => `Re-read ${dir}/SKILL.md and each file named above before you rely on them.`;

test("a Session with no baseline records one and says nothing", async (t) => {
  const f = fixture(t);
  await start(f);
  const entries = skillEntries(f);
  assert.equal(entries.length, 1);
  assert.deepEqual(entries[0].data, { skill: "cpd-lead", files: { "SKILL.md": sha(DEFAULT("cpd-lead")) } });
  assert.deepEqual(notices(await turn(f)), []);
});

test("unchanged skill files at a later start add no entry and no message", async (t) => {
  const f = fixture(t);
  await start(f);
  assert.equal(skillEntries(f).length, 1, "the first start recorded a baseline");
  await start(f, "reload");
  assert.equal(skillEntries(f).length, 1);
  assert.deepEqual(notices(await turn(f)), []);
});

test("a changed file is named in one cpd-skills message at the next turn and no turn starts", async (t) => {
  const f = fixture(t);
  await start(f);
  assert.equal(skillEntries(f).length, 1, "the first start recorded a baseline");
  write(f, "cpd-lead", "SKILL.md", "changed text\n");
  await start(f, "reload");
  assert.deepEqual(notices(f.pi.sent.map((s) => s.message)), [], "nothing is sent before a turn");
  assert.equal(skillEntries(f).length, 1, "no entry yet");

  const ms = notices(await turn(f));
  const dir = dirOf(f, "cpd-lead");
  assert.equal(ms.length, 1);
  assert.equal(ms[0].display, true);
  assert.equal(ms[0].content, `Skill cpd-lead changed: SKILL.md modified. ${ending(dir)}`);
  assert.deepEqual(ms[0].details, { skill: "cpd-lead", dir, changes: [{ file: "SKILL.md", change: "modified" }] });
  assert.deepEqual(notices(await turn(f)), []);
  assert.deepEqual(f.pi.sent.filter((s) => s.message.customType === "cpd-skills" && (s.options as any).triggerTurn), []);
});

test("added and removed files are named in path order", async (t) => {
  const f = fixture(t);
  write(f, "cpd-lead", "references/old.md", "old\n");
  await start(f);
  assert.equal(skillEntries(f).length, 1, "the first start recorded a baseline");
  remove(f, "cpd-lead", "references/old.md");
  write(f, "cpd-lead", "references/new.md", "new\n");
  write(f, "cpd-lead", "references/a.md", "a\n");
  await start(f, "reload");
  const ms = notices(await turn(f));
  assert.equal(ms.length, 1);
  assert.equal(ms[0].content, `Skill cpd-lead changed: references/a.md added, references/new.md added, references/old.md removed. ${ending(dirOf(f, "cpd-lead"))}`);
  assert.deepEqual(ms[0].details.changes, [
    { file: "references/a.md", change: "added" },
    { file: "references/new.md", change: "added" },
    { file: "references/old.md", change: "removed" },
  ]);
});

test("the notice moves the baseline so the next start says nothing", async (t) => {
  const f = fixture(t);
  await start(f);
  write(f, "cpd-lead", "SKILL.md", "changed text\n");
  await start(f, "reload");
  assert.equal(notices(await turn(f)).length, 1);
  const last = skillEntries(f).at(-1);
  assert.deepEqual(last?.data, { skill: "cpd-lead", files: { "SKILL.md": sha("changed text\n") } });
  await start(f, "reload");
  assert.deepEqual(notices(await turn(f)), []);
});

test("a notice not delivered before a reload comes again with the newer changes", async (t) => {
  const f = fixture(t);
  await start(f);
  assert.equal(skillEntries(f).length, 1, "the first start recorded a baseline");
  write(f, "cpd-lead", "SKILL.md", "changed text\n");
  await start(f, "reload");
  write(f, "cpd-lead", "references/x.md", "x\n");
  await start(f, "reload");
  assert.equal(skillEntries(f).length, 1, "no baseline beyond the first before a turn");
  const ms = notices(await turn(f));
  assert.equal(ms.length, 1);
  assert.equal(ms[0].content, `Skill cpd-lead changed: SKILL.md modified, references/x.md added. ${ending(dirOf(f, "cpd-lead"))}`);
});

test("a Manager is told of cpd-manager and a Lead of cpd-lead and a Worker of nothing", async (t) => {
  const m = fixture(t, "manager");
  await start(m);
  assert.equal(skillEntries(m).length, 1, "the first start recorded a baseline");
  assert.equal(skillEntries(m)[0].data.skill, "cpd-manager");
  write(m, "cpd-manager", "SKILL.md", "changed text\n");
  await start(m, "reload");
  const [note] = notices(await turn(m));
  assert.equal(note.details.skill, "cpd-manager");
  assert.equal(note.details.dir, dirOf(m, "cpd-manager"));

  const l = fixture(t, "lead");
  await start(l);
  assert.equal(skillEntries(l).length, 1, "the first start recorded a baseline");
  write(l, "cpd-lead", "SKILL.md", "changed text\n");
  await start(l, "reload");
  const [lnote] = notices(await turn(l));
  assert.equal(lnote.details.skill, "cpd-lead");
  assert.equal(lnote.details.dir, dirOf(l, "cpd-lead"));

  const w = fixture(t, "worker");
  rmSync(join(w.adapterDir, "skills"), { recursive: true, force: true });
  await start(w);
  await start(w, "reload");
  assert.deepEqual(skillEntries(w), []);
  assert.deepEqual(skillReports(w), []);
  assert.deepEqual(w.pi.errors(), []);
});

test("an unreadable skills directory is reported to the ledger for a Lead and for a Manager", async (t) => {
  for (const role of ["lead", "manager"] as const) {
    for (const how of ["no directory", "no SKILL.md"]) {
      const f = fixture(t, role);
      if (how === "no directory") rmSync(join(f.adapterDir, "skills"), { recursive: true, force: true });
      else remove(f, SKILL[role], "SKILL.md");
      await f.pi.fire("session_start", { reason: "resume" });
      await f.followerStarted();
      await f.quiet();
      assert.equal(skillReports(f).length, 1);
      const [r] = skillReports(f);
      assert.equal(r!.what, "skills check");
      assert.match(r!.message ?? "", /^cannot read |holds no SKILL\.md$/);
      assert.deepEqual(skillEntries(f), []);
      if (role === "manager") assert.ok(f.pi.errors().length >= 1, "a Manager also gets the chat error");

      await f.pi.fire("session_start", { reason: "reload" });
      await f.quiet();
      assert.equal(skillReports(f).length, 2);
    }
  }
});

test("an entry that does not parse is reported as entry_invalid and replaced by a baseline", async (t) => {
  const f = fixture(t);
  f.pi.entries = [{ type: "custom", customType: "cpd-skills", data: { skill: "cpd-lead", files: { "SKILL.md": "nothex" } } }];
  await f.pi.fire("session_start", { reason: "resume" });
  await f.quiet();
  assert.equal(skillReports(f, "entry_invalid").length, 1);
  assert.deepEqual(notices(await turn(f)), []);
  assert.equal(skillEntries(f).length, 2, "a baseline was appended after the invalid entry");
  assert.deepEqual(skillEntries(f).at(-1)?.data, { skill: "cpd-lead", files: { "SKILL.md": sha(DEFAULT("cpd-lead")) } });
});

test("the cpd-skills type has a registered renderer", async (t) => {
  const f = fixture(t);
  assert.ok(f.pi.renderers.has("cpd-skills"));
});

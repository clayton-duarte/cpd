import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { Fixture, shown } from "./fake.ts";

// Decision pro-d56, commands.md §15.20: a Lead runs a command the User explicitly asks for.

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
const lead = read("../skills/cpd-lead/SKILL.md");
const manager = read("../skills/cpd-manager/SKILL.md");
const doc = read("../../docs/commands.md");
const HEADING = "## A command the User asks for";

function section(): string {
  const lines = lead.split("\n");
  const start = lines.indexOf(HEADING);
  assert.notEqual(start, -1, `cpd-lead SKILL.md lacks the heading line "${HEADING}"`);
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => l.startsWith("## "));
  return [lines[start], ...(end === -1 ? rest : rest.slice(0, end))].join("\n");
}

/** Fenced blocks (empty info string) of the section, indent of two spaces stripped, one entry per block. */
function blocks(text: string): string[][] {
  const out: string[][] = [];
  let cur: string[] | undefined;
  for (const raw of text.split("\n")) {
    const l = raw.replace(/^ {2}/, "");
    if (l.trimEnd() === "```") {
      if (cur) { out.push(cur); cur = undefined; } else cur = [];
    } else if (cur) cur.push(l);
  }
  return out;
}

const startBlocks = (s: string) => blocks(s).filter((b) => b.some((l) => l.includes("set -m")));
const stopBlocks = (s: string) => blocks(s).filter((b) => b.some((l) => l.includes("kill -TERM")));

const count = (text: string, needle: string) => text.split(needle).length - 1;

test("cpd-lead skill: a Lead runs a command the User explicitly asks for, stated once (pro-d56)", () => {
  assert.equal(lead.split("\n").filter((l) => l === HEADING).length, 1, `exactly one "${HEADING}" line`);
  assert.ok(lead.indexOf(HEADING) < lead.indexOf("## 1. Context"), "the section sits before ## 1. Context");
  const s = section();
  for (const str of [
    "explicitly asks you to run a command", "pro-d56", "commands.md §15.20", "Only an explicit request counts.", "Never infer one",
    "What stays.", "lead_worktree", "in the background", "URL or port", "the log path", "how to stop it", "Leave nothing orphaned.", "plan-done",
  ]) assert.ok(s.includes(str), `the section lacks "${str}"`);

  const lines = lead.split("\n");
  const act = lines.find((l) => l.startsWith("- Act "));
  assert.ok(act, "a Rules bullet starts `- Act `");
  assert.ok(!act.startsWith("- Act only through"), "the Act bullet must not start `- Act only through`");
  assert.ok(act.includes("(#a-command-the-user-asks-for)"), "the Act bullet links the section");
  const stay = lines.find((l) => l.startsWith("- **Stay free for the User.**"));
  assert.ok(stay, "the Stay free bullet exists");
  assert.ok(stay.includes("Never poll, sleep or run a foreground loop. The adapter refuses a bash call of yours that waits longer than 30 seconds."), "the Stay free bullet keeps its sentence");
  assert.ok(stay.includes("(#a-command-the-user-asks-for)"), "the Stay free bullet links the section");

  assert.equal(count(lead, "explicitly asks you to run"), 1, "the phrase occurs once in the Lead SKILL.md");
  const refs = new URL("../skills/cpd-lead/references/", import.meta.url);
  for (const f of readdirSync(refs)) assert.equal(count(readFileSync(new URL(f, refs), "utf8"), "explicitly asks you to run"), 0, `references/${f} must not restate the rule`);

  assert.ok(manager.split("\n").some((l) => l.startsWith("- Act only through `cpd` commands and chat with the User")), "the Manager keeps `- Act only through `cpd` commands and chat with the User`");
  assert.ok(!manager.includes("explicitly asks you to run"), "the Manager skill must not carry the rule");

  assert.ok(doc.split("\n").some((l) => l.startsWith("### 15.20 A Lead runs a command the User explicitly asks for")), "commands.md has section 15.20");
});

test("cpd-lead skill: a long-lived command runs in the background with a log, a pid file and a stop command (pro-d56)", () => {
  const s = section();
  const st = startBlocks(s);
  const sp = stopBlocks(s);
  assert.equal(st.length, 1, "exactly one START block (with `set -m`) in the section");
  assert.equal(sp.length, 1, "exactly one STOP block (with `kill -TERM`) in the section");
  assert.equal(blocks(s).length, 2, "exactly two fenced blocks in the section");
  assert.equal(st[0].length, 1, "the START block is a single line");
  assert.equal(sp[0].length, 1, "the STOP block is a single line");
  for (const str of ["set -m", "> /tmp/cpd-<plan>/<name>.log 2>&1", "echo $! > /tmp/cpd-<plan>/<name>.pid", "<plan>", "<dir>", "<command>", "<name>"])
    assert.ok(st[0][0].includes(str), `the START line lacks "${str}"`);
  assert.ok(sp[0][0].includes("kill -TERM -- -$(cat /tmp/cpd-<plan>/<name>.pid)"), "the STOP line lacks the group kill");
});

test("a Lead's background start and stop pass the adapter's guards and the guards still refuse what stays (pro-d56)", async (t) => {
  const s = section();
  const st = startBlocks(s)[0]?.[0];
  const sp = stopBlocks(s)[0]?.[0];
  assert.ok(st, "the START block is missing from the section");
  assert.ok(sp, "the STOP block is missing from the section");
  const fill = (l: string) => l.replaceAll("<plan>", "p1").replaceAll("<dir>", "/tmp/cpd-p1-wt").replaceAll("<command>", "npm run dev").replaceAll("<name>", "dev");

  const primary = mkdtempSync(join(tmpdir(), "uc-primary-"));
  t.after(() => rmSync(primary, { recursive: true, force: true }));
  mkdirSync(join(primary, ".git"));

  const f = new Fixture();
  f.respond("session-show", shown({ role: "lead", plan: "p1", plan_lead: true }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  t.after(() => f.close());

  const call = async (command: string) => (await f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: "t", input: { command } }))[0] as { block?: boolean } | undefined;
  assert.equal(await call(fill(st)), undefined, "the START line must run");
  assert.equal(await call(fill(sp)), undefined, "the STOP line must run");
  for (const c of ["op item get x", "sleep 60", "timeout 120 npm run dev", `git -C ${primary} checkout main`])
    assert.equal((await call(c))?.block, true, `${c} must stay refused`);
});

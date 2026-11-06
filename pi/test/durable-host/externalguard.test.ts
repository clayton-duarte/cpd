import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { beforeEach, test } from "node:test";
import { registerHostTools, type RegisterHostToolsWhere } from "../../durable-host/tools.ts";
import { hostVerdict, type Where } from "../../durable-host/guards.ts";
import { grantSlackSend, resetSlackGrants } from "../../lib/externalguard.ts";

// Decision pro-d451 in the durable host: hostVerdict for lead and manager, and the tool registration.

beforeEach(() => resetSlackGrants());
const ROLES = ["lead", "manager"] as const;
const SEND = "mcp__slack__slack_send_message";
const where = (): Where => {
  const tmp = mkdtempSync(join(tmpdir(), "durable-host-external-"));
  return { cwd: tmp, home: tmp, visible: [] };
};
const blocked = (v: unknown, what: string) => {
  const b = (v as { block?: unknown } | undefined)?.block;
  assert.ok(typeof b === "string" && b !== "" && !b.includes("\n") && b.includes("cpd_external_approve"), `${what}: ${JSON.stringify(v)}`);
};
const bash = (command: string) => ({ id: "c", name: "bash", arguments: { command, timeout: 5 } });

test("the durable host refuses raw gh comment writes for lead and manager", (t) => {
  const w = where();
  t.after(() => rmSync(w.cwd, { recursive: true, force: true }));
  for (const role of ROLES) {
    for (const command of ["gh pr comment 1 -b x", "timeout 30 gh pr review 1 --approve", "gh api repos/o/r/pulls/1/reviews -X POST -f event=COMMENT"]) blocked(hostVerdict(bash(command), w, role), `${role} ${command}`);
    assert.equal(hostVerdict(bash("gh pr view 1"), w, role), undefined, `${role}: a read runs`);
  }
});

test("the durable host refuses an ungranted slack send and lets one granted send through once", (t) => {
  const w = where();
  t.after(() => rmSync(w.cwd, { recursive: true, force: true }));
  for (const role of ROLES) {
    const call = { id: "c", name: SEND, arguments: { channel_id: "C1", message: "hi" } };
    blocked(hostVerdict(call, w, role), `${role} ungranted`);
    blocked(hostVerdict({ ...call, name: "mcp__slack__slack_send_message_draft" }, w, role), `${role} draft`);
    grantSlackSend("slack_send_message", { channel: "C1", message: "hi" });
    assert.equal(hostVerdict(call, w, role), undefined, `${role}: granted passes`);
    blocked(hostVerdict(call, w, role), `${role} second`);
    blocked(hostVerdict({ ...call, parent: { taskId: "t", callId: "p" } }, w, role), `${role} nested`);
  }
});

test("the durable host refuses cpd pr approve and slack-post.ts in bash", (t) => {
  const w = where();
  t.after(() => rmSync(w.cwd, { recursive: true, force: true }));
  for (const role of ROLES) {
    for (const command of ["cpd pr approve --kind post --workflow w", "node /x/slack-post.ts --channel C1", "/x/slack-post.ts hi"]) blocked(hostVerdict(bash(command), w, role), `${role} ${command}`);
  }
});

test("the durable host registers cpd_external_approve", async () => {
  const records: string[][] = [];
  const where: RegisterHostToolsWhere = {
    role: "lead",
    plan: "p1",
    name: "s-lead",
    cpd: async (args) => {
      records.push([...args]);
      return { ok: true };
    },
    held: { create: () => ({ id: "i", promise: new Promise<object | undefined>(() => {}) }), answer: () => ({ outcome: "answered" }), resolveByHost: () => {}, resolveAll: () => {}, heldCount: () => 0, heldList: () => [] },
    dialogs: {
      select: async () => "Allow once",
      confirm: async () => true,
      input: async () => "",
      editor: async () => "",
      questionnaire: async () => ({ answers: [], cancelled: true }),
      notice: () => {},
      status: () => {},
    },
  };
  const tool = registerHostTools(where).tools?.find((x) => x.name === "cpd_external_approve") as unknown as { execute(a: unknown, b: unknown, c: unknown): Promise<unknown> } | undefined;
  assert.ok(tool, "the tool cpd_external_approve is registered");
  await tool.execute({ kind: "pr_post", workflow: "wf-1", reason: "r" }, {}, {});
  assert.deepEqual(records, [["pr", "approve", "--kind", "post", "--workflow", "wf-1"]]);
});

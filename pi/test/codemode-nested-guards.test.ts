import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { Fixture, shown } from "./fake.ts";

// commands.md §15.21 (hs-d736): a call a codemode script makes (toolCallId `<parent>/<n>`, parentToolCallId) is judged like a direct one.

type Verdict = { block?: boolean; reason?: string } | undefined;

async function started(role: string) {
  const f = new Fixture();
  f.respond("session-show", shown({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead" }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  return f;
}

const fire = async (f: Fixture, toolName: string, input: unknown, nested: boolean): Promise<Verdict> =>
  (await f.pi.fireWith("tool_call", nested ? { toolName, toolCallId: "p/1", parentToolCallId: "p", input } : { toolName, toolCallId: "p", input }))[0] as Verdict;

/** The direct verdict is a block with a one-line reason, and the nested one is deeply equal to it. */
async function same(f: Fixture, what: string, toolName: string, input: unknown): Promise<void> {
  const direct = await fire(f, toolName, input, false);
  assert.equal(direct?.block, true, `${what}: direct is refused`);
  assert.ok(typeof direct?.reason === "string" && direct.reason !== "" && !direct.reason.includes("\n"), `${what}: a one-line reason`);
  assert.deepEqual(await fire(f, toolName, input, true), direct, `${what}: nested`);
}

/** A primary checkout (a `.git` directory) in a temporary home-less directory; nothing runs git. */
function primary(t: TestContext): string {
  const tmp = mkdtempSync(join(tmpdir(), "codemode-nested-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const root = join(tmp, "projects", "repo");
  mkdirSync(join(root, ".git"), { recursive: true });
  mkdirSync(join(root, "docs"));
  return root;
}

const inCwd = (f: Fixture, dir: string) => {
  const ctx = f.pi.ctx.bind(f.pi);
  f.pi.ctx = () => ({ ...ctx(), cwd: dir });
};

test("a nested edit or write into a primary checkout is refused like a direct one", async (t) => {
  const root = primary(t);
  for (const role of ["lead", "manager", "worker"]) {
    const f = await started(role);
    t.after(() => f.close());
    inCwd(f, tmpdir());
    for (const tool of ["edit", "write"]) await same(f, `${role} ${tool}`, tool, { path: join(root, "docs", "x.md") });
  }
});

test("a nested bash that moves HEAD of a primary checkout is refused like a direct one", async (t) => {
  const root = primary(t);
  for (const role of ["lead", "manager", "worker"]) {
    const f = await started(role);
    t.after(() => f.close());
    inCwd(f, tmpdir());
    await same(f, role, "bash", { command: `git -C ${root} checkout -q --detach origin/main` });
  }
});

test("a nested bash that calls op or a host approval directly is refused like a direct one", async (t) => {
  for (const role of ["lead", "manager", "worker"]) {
    const f = await started(role);
    t.after(() => f.close());
    for (const command of ["op read op://vault/item/field", "cpd secret approve", "cpd host approve"]) await same(f, `${role} ${command}`, "bash", { command });
  }
});

test("a nested bash wait is refused for a Lead and a Manager like a direct one", async (t) => {
  for (const role of ["lead", "manager"]) {
    const f = await started(role);
    t.after(() => f.close());
    for (const command of ["sleep 60", "tail -f log"]) await same(f, `${role} ${command}`, "bash", { command });
  }
});

test("a nested full test run is refused for a Worker like a direct one", async (t) => {
  const f = await started("worker");
  t.after(() => f.close());
  for (const command of ["scripts/cargo.sh test --workspace", "npm test"]) await same(f, command, "bash", { command });
});

const question = (text: string) => ({ questions: [{ question: text, header: "h", options: [{ label: "A1", description: "d" }, { label: "A3", description: "d" }] }] });

test("a nested ask_user_question over the limit is refused like a direct one", async (t) => {
  const f = await started("lead");
  t.after(() => f.close());
  f.pi.entries = [{ type: "message", message: { role: "assistant", content: [{ type: "text", text: "Here is what I think." }, { type: "toolCall", id: "p", name: "codemode", arguments: {} }] } }];
  await same(f, "over the limit", "ask_user_question", question("q".repeat(1600)));
});

test("a nested ask_user_question citing a label stated nowhere is refused like a direct one", async (t) => {
  const f = await started("lead");
  t.after(() => f.close());
  f.pi.entries = [
    {
      type: "message",
      message: {
        role: "assistant",
        content: [
          { type: "text", text: "Here is what I think." },
          { type: "toolCall", id: "p", name: "codemode", arguments: {} },
          { type: "text", text: "A2 is stated here, after the call" },
        ],
      },
    },
  ];
  await same(f, "A2", "ask_user_question", question("Pick A1–A3?"));
});

test("a nested call that no guard refuses is not blocked", async (t) => {
  for (const role of ["lead", "manager", "worker"]) {
    const f = await started(role);
    t.after(() => f.close());
    for (const [tool, input] of [["read", { path: "README.md" }], ["bash", { command: "ls" }]] as const) {
      assert.equal(await fire(f, tool, input, true), undefined, `${role} ${tool}`);
    }
  }
});

import assert from "node:assert/strict";
import { test } from "node:test";
import { electronBypass, reasonText } from "../lib/electronguard.ts";
import { Fixture, shown, waitFor } from "./fake.ts";

// commands.md §15.28: no agent runs the electron binary by hand.

test("electronBypass refuses the electron binary as a command word, after prefixes and as a path", () => {
  const cases: [string, string][] = [
    ["electron .", "electron"],
    ["electron", "electron"],
    ["FOO=1 electron .", "electron"],
    ["sudo env electron .", "electron"],
    ["timeout 5 electron .", "electron"],
    ["/usr/local/bin/electron .", "/usr/local/bin/electron"],
    ["cd x && electron .", "electron"],
  ];
  for (const [command, matched] of cases) assert.equal(electronBypass(command), matched, command);
});

test("electronBypass refuses npx, pnpm, npm and yarn running electron", () => {
  const cases: [string, string][] = [
    ["npx electron", "npx electron"],
    ["npx -y electron@30 .", "npx electron@30"],
    ["pnpm exec electron .", "pnpm exec electron"],
    ["pnpm --filter cpd-web-client exec electron .", "pnpm --filter cpd-web-client exec electron"],
    ["npm exec electron", "npm exec electron"],
    ["yarn dlx electron", "yarn dlx electron"],
  ];
  for (const [command, matched] of cases) assert.equal(electronBypass(command), matched, command);
});

test("electronBypass refuses the Electron.app binary path", () => {
  const p = "/Applications/Electron.app/Contents/MacOS/Electron";
  assert.equal(electronBypass(p), p);
  assert.equal(electronBypass(`${p} .`), p);
});

test("electronBypass passes cpd app, build scripts and electron as a plain argument", () => {
  const passes = [
    "echo electron",
    "grep electron f",
    "cat web/electron.ts",
    "pnpm run build:electron",
    "pnpm --filter cpd-web-client run build:electron",
    "ls node_modules/.bin/electron-builder",
    "cpd app",
    "cpd app --status",
    "sh -c 'electron .'",
  ];
  for (const c of passes) assert.equal(electronBypass(c), undefined, c);
  assert.equal(electronBypass("echo electron; electron ."), "electron"); // only the second command is judged
});

test("the electron reason is one line naming cpd app and its four flags", () => {
  assert.equal(
    reasonText("pnpm exec electron"),
    "φ: agents do not run the electron binary; use cpd app (--preview, --reload, --status, --off) (commands.md §15.28); refused: pnpm exec electron",
  );
  assert.ok(!reasonText("electron").includes("\n"));
});

type Result = { block?: boolean; reason?: string } | undefined;

test("the adapter blocks a bash electron call of every role", async (t) => {
  for (const role of ["lead", "manager", "worker"]) {
    const f = new Fixture();
    t.after(() => f.close());
    f.respond("session-show", shown({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead" }));
    f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
    f.load();
    await f.pi.fire("session_start");
    await waitFor("the follower", () => f.follows().length > 0);
    const call = async (command: string) => (await f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: "t", input: { command } }))[0] as Result;
    const r = await call("electron .");
    assert.equal(r?.block, true, role);
    assert.equal(r?.reason, reasonText("electron"), role);
    assert.equal(await call("cpd app --status"), undefined, role);
  }
});

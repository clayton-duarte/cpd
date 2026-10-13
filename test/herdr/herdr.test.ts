import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { herdr } from "../../src/herdr/herdr.ts";
import { agentGet, promptAgent, reviewerArgs, startAgent, workerArgs, type StartAgent } from "../../src/herdr/agents.ts";
import { closePane, closeWorkspace, paneGet, rootPaneOfWorkspace, splitPane } from "../../src/herdr/panes.ts";
import { openWorktree, workspaceForWorktree } from "../../src/herdr/worktrees.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { tmpDir } from "../helpers/index.ts";
import { says, useFakeHerdr } from "./setup.ts";

const pane = (id: string, extra: object = {}) => ({ pane_id: id, workspace_id: id.split(":")[0], cwd: "/x", ...extra });
const ok = (result: unknown) => says({ id: "cli:x", result });
const agent = (extra: object = {}) => ({
  agent: "pi", name: "c1", pane_id: "w1:p2", agent_status: "idle",
  agent_session: { agent: "pi", kind: "path", source: "herdr:pi", value: "/s/session.jsonl" }, ...extra,
});

const rejects = (p: Promise<unknown>, code: string, exitCode = 1) =>
  assert.rejects(p, (e: unknown) => e instanceof CpdError && e.code === code && e.exitCode === exitCode, code);

test("splitPane passes the exact argv (--no-focus, env) and returns the pane id", async () => {
  const h = useFakeHerdr(ok({ type: "pane_info", pane: pane("w1:p2") }));
  const id = await splitPane("w1:p1", { direction: "right", cwd: "/wt", env: { A: "1", B: "x y" }, noFocus: true });
  assert.equal(id, "w1:p2");
  assert.deepEqual(h.calls(), [["pane", "split", "w1:p1", "--direction", "right", "--cwd", "/wt", "--env", "A=1", "--env", "B=x y", "--no-focus"]]);
});

test("closePane, paneGet, closeWorkspace and rootPaneOfWorkspace use the documented subcommands", async () => {
  const h = useFakeHerdr(`case "$1 $2" in
"pane close"|"workspace close") ${ok({ type: "ok" })};;
"pane get") ${ok({ type: "pane_info", pane: pane("w1:p1", { agent_session: { kind: "path", value: "/s/lead.jsonl" } }) })};;
"pane list") ${ok({ type: "pane_list", panes: [pane("w3:p1"), pane("w3:p2")] })};;
esac`);
  await closePane("w1:p2");
  assert.equal((await paneGet("w1:p1")).agent_session?.value, "/s/lead.jsonl");
  assert.equal(await rootPaneOfWorkspace("w3"), "w3:p1");
  await closeWorkspace("w3");
  assert.deepEqual(h.calls(), [
    ["pane", "close", "w1:p2"],
    ["pane", "get", "w1:p1"],
    ["pane", "list", "--workspace", "w3"],
    ["workspace", "close", "w3"],
  ]);
});

test("closeWorkspace surfaces workspace_group_close_required as herdr_workspace_group_close_required", async () => {
  useFakeHerdr(`${says({ id: "x", error: { code: "workspace_group_close_required", message: "close children first" } })}; exit 1`);
  await rejects(closeWorkspace("w2"), "herdr_workspace_group_close_required");
});

test("openWorktree passes the exact argv and returns the workspace id", async () => {
  const h = useFakeHerdr(ok({ type: "worktree_opened", already_open: false, workspace: { workspace_id: "w3", label: "wf job a1" } }));
  assert.equal(await openWorktree({ repo: "/repo", path: "/wt", label: "wf job a1" }), "w3");
  assert.deepEqual(h.calls(), [["worktree", "open", "--cwd", "/repo", "--path", "/wt", "--label", "wf job a1", "--no-focus"]]);
});

test("workspaceForWorktree matches on canonical checkout path and repo; null when none", async () => {
  const root = tmpDir();
  const repo = join(root, "repo");
  const wt = join(root, "wt");
  mkdirSync(repo);
  mkdirSync(wt);
  const w = (id: string, checkout: string, repoRoot: string) => ({
    workspace_id: id, worktree: { checkout_path: checkout, repo_root: repoRoot, is_linked_worktree: true },
  });
  const h = useFakeHerdr(ok({ type: "workspace_list", workspaces: [{ workspace_id: "w1" }, w("w2", join(root, "other"), repo), w("w3", wt, repo)] }));
  assert.equal(await workspaceForWorktree(repo, wt), "w3");
  assert.equal(await workspaceForWorktree(join(root, "elsewhere"), wt), null);
  assert.equal(await workspaceForWorktree(repo, join(root, "missing")), null);
  assert.deepEqual(h.calls()[0], ["workspace", "list"]);
});

test("startAgent (Worker): exact argv; returns herdr's argv and session path", async () => {
  const h = useFakeHerdr(ok({ type: "agent_started", agent: agent(), argv: ["pi", "--approve", "--model", "m/x:low"] }));
  const r = await startAgent({ name: "c3f9a0b1c2d4e", kind: "pi", pane: "w3:p2", timeoutMs: 60000, args: workerArgs("m/x:low") });
  assert.deepEqual(r, { argv: ["pi", "--approve", "--model", "m/x:low"], sessionPath: "/s/session.jsonl" });
  assert.deepEqual(h.calls(), [["agent", "start", "c3f9a0b1c2d4e", "--kind", "pi", "--pane", "w3:p2", "--timeout", "60000", "--", "--approve", "--model", "m/x:low"]]);
});

test("startAgent (Reviewer): fork, read-only tools; fresh mode drops --fork", async () => {
  const h = useFakeHerdr(ok({ type: "agent_started", agent: agent(), argv: ["pi"] }));
  await startAgent({ name: "cr1", kind: "pi", pane: "w1:p3", timeoutMs: 60000, args: reviewerArgs("/lead.jsonl", "m/x:high") });
  assert.deepEqual(h.calls()[0]!.slice(10), ["--approve", "--fork", "/lead.jsonl", "--tools", "read,grep,find,ls", "--model", "m/x:high"]);
  assert.deepEqual(reviewerArgs(null, "m/x:high"), ["--approve", "--tools", "read,grep,find,ls", "--model", "m/x:high"]);
  assert.deepEqual(workerArgs("m/x:low"), ["--approve", "--model", "m/x:low"]);
});

test("startAgent refuses bad names, control characters and bad timeouts before calling herdr", async () => {
  const h = useFakeHerdr(ok({}));
  const base: StartAgent = { name: "c1", kind: "pi", pane: "w1:p1", timeoutMs: 60000, args: [] };
  await rejects(startAgent({ ...base, name: "Bad Name" }), "invalid_agent_name", 2);
  await rejects(startAgent({ ...base, args: ["--model", "a\nb"] }), "invalid_herdr_argument");
  await rejects(startAgent({ ...base, args: ["a\u0000b"] }), "invalid_herdr_argument");
  await rejects(startAgent({ ...base, timeoutMs: 300001 }), "invalid_timeout", 2);
  assert.deepEqual(h.calls(), []);
});

test("promptAgent always passes --wait, and --timeout only when given; text is one argv element", async () => {
  const h = useFakeHerdr(ok({ type: "agent_prompted", agent: agent({ agent_status: "done" }) }));
  const r = await promptAgent("c1", "Read /spec.md\nthen --follow it", { wait: true });
  assert.equal(r.agent_status, "done");
  await promptAgent("c1", "again", { wait: true, timeoutMs: 5000 });
  assert.deepEqual(h.calls(), [
    ["agent", "prompt", "c1", "Read /spec.md\nthen --follow it", "--wait"],
    ["agent", "prompt", "c1", "again", "--wait", "--timeout", "5000"],
  ]);
});

test("agentGet returns the agent; agent_not_found gives null", async () => {
  const h = useFakeHerdr(ok({ type: "agent_info", agent: agent() }));
  assert.equal((await agentGet("c1"))?.pane_id, "w1:p2");
  assert.deepEqual(h.calls(), [["agent", "get", "c1"]]);
  useFakeHerdr(`${says({ id: "x", error: { code: "agent_not_found", message: "agent target c1 not found" } })}; exit 1`);
  assert.equal(await agentGet("c1"), null);
});

test("agentGet passes other herdr errors through; server_not_running (on stderr) is herdr_server_not_running, exit 1", async () => {
  useFakeHerdr(`${says({ id: "x", error: { code: "server_not_running", message: "no herdr server is running" } })} >&2; exit 1`);
  await rejects(agentGet("c1"), "herdr_server_not_running", 1);
  useFakeHerdr(`${says({ id: "x", error: { code: "pane_not_found", message: "nope" } })}; exit 1`);
  await rejects(agentGet("c1"), "herdr_pane_not_found");
});

test("--session is injected first when config.herdrSession is set, and absent when null", async () => {
  const withSession = useFakeHerdr(ok({ type: "ok" }), "cpd-test");
  await closePane("w1:p2");
  assert.deepEqual(withSession.calls(), [["--session", "cpd-test", "pane", "close", "w1:p2"]]);
  const without = useFakeHerdr(ok({ type: "ok" }), null);
  await closePane("w1:p2");
  assert.deepEqual(without.calls(), [["pane", "close", "w1:p2"]]);
});

test("garbage output, a missing result and a wrong shape fail loud as bad_tool_output", async () => {
  useFakeHerdr("echo not json");
  await rejects(herdr(["x"], z.unknown()), "bad_tool_output");
  useFakeHerdr("echo 1; exit 1");
  await rejects(herdr(["x"], z.unknown()), "bad_tool_output");
  useFakeHerdr(ok({ type: "pane_info" }));
  await rejects(paneGet("w1:p1"), "bad_tool_output");
  useFakeHerdr(`${says({ id: "x", result: { type: "ok" } })}; exit 3`);
  await rejects(herdr(["x"], z.unknown()), "bad_tool_output");
});

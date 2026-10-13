import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { agentGet, promptAgent, startAgent, workerArgs } from "../../src/herdr/agents.ts";
import { closePane, closeWorkspace, paneGet, rootPaneOfWorkspace, splitPane } from "../../src/herdr/panes.ts";
import { lastAssistant, readSessionHeader } from "../../src/herdr/session.ts";
import { openWorktree, workspaceForWorktree } from "../../src/herdr/worktrees.ts";
import { sandboxEnv, tmpDir } from "../helpers/index.ts";
import { baseConfig } from "./setup.ts";

/**
 * Needs a running `herdr --session cpd-test server` and a real pi model: only with CPD_IT=1.
 * Uses only the `cpd-test` herdr session, never the default one.
 */
test("real herdr: split, start a Worker at haiku:low, prompt it to create a file, then clean up", { skip: process.env.CPD_IT !== "1", timeout: 180_000 }, async () => {
  const e = sandboxEnv();
  mkdirSync(e.CPD_HOME!, { recursive: true });
  writeFileSync(join(e.CPD_HOME!, "config.json"), JSON.stringify({ ...baseConfig, herdrSession: "cpd-test" }));
  process.env.CPD_HOME = e.CPD_HOME; // HOME stays real: herdr finds its sockets under it

  const root = realpathSync(tmpDir("cpd-it-"));
  const repo = join(root, "repo");
  const wt = join(root, "wt");
  const git = (...a: string[]) => {
    const r = spawnSync("git", ["-C", repo, ...a], { encoding: "utf8" });
    assert.equal(r.status, 0, r.stderr);
  };
  mkdirSync(repo);
  spawnSync("git", ["init", "-q", "-b", "main", repo]);
  git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init");
  git("worktree", "add", "-q", "-b", "it-branch", wt);

  const ws = await openWorktree({ repo, path: wt, label: "cpd it a1" });
  try {
    assert.equal(await openWorktree({ repo, path: wt, label: "cpd it a1" }), ws, "worktree open is idempotent");
    assert.equal(await workspaceForWorktree(repo, wt), ws);
    const rootPane = await rootPaneOfWorkspace(ws);
    const pane = await splitPane(rootPane, { direction: "right", cwd: wt, noFocus: true });
    const name = "cpditworker1";
    assert.equal(await agentGet(name), null);

    const started = await startAgent({ name, kind: "pi", pane, timeoutMs: 60_000, args: workerArgs("github-copilot/claude-haiku-4.5:low") });
    assert.deepEqual(started.argv, ["pi", "--approve", "--model", "github-copilot/claude-haiku-4.5:low"]);
    assert.equal(existsSync(started.sessionPath), false, "the session file appears only with the first message (t1 F10)");
    assert.equal((await agentGet(name))?.pane_id, pane);
    assert.equal((await paneGet(pane)).agent_session?.value, started.sessionPath);

    await promptAgent(name, "Create a file hello.txt containing 1 in the current directory, then stop.", { wait: true });
    assert.equal(readFileSync(join(wt, "hello.txt"), "utf8").trim(), "1");
    assert.equal(readSessionHeader(started.sessionPath).cwd, wt);
    assert.equal(lastAssistant(started.sessionPath)?.stopReason, "stop");
    const levels = readFileSync(started.sessionPath, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)).filter((x) => x.type === "thinking_level_change");
    assert.deepEqual(levels.map((l) => l.thinkingLevel), ["low"]);

    await closePane(pane);
    assert.equal(existsSync(join(wt, "hello.txt")), true);
  } finally {
    await closeWorkspace(ws).catch(() => {});
    const primary = await workspaceForWorktree(repo, repo);
    if (primary) await closeWorkspace(primary).catch(() => {});
  }
});

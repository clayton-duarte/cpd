import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/**
 * A scripted stand-in for the `herdr` binary, so the whole lifecycle can run against the real Ledger, git and wt
 * without a herdr server or a model. Put it first on PATH. State lives in $FAKE_HERDR_DIR:
 *   scenario.json  {<attempt id>: {worker: ("submit"|"idle"|"crash")[], reviewer: ("approve"|"reject"|"garbage")[], startFails?: boolean}}
 *                  consumed one entry per prompt; an Attempt without an entry gets the defaults (idle Worker, garbage Reviewer)
 *   calls.jsonl    every argv it was called with
 *   sessions/      pi-style session files it writes for each agent
 * A "submit" Worker really commits a file in its worktree and runs `cpd attempt submit` ($FAKE_CPD, $FAKE_WF).
 */
const SOURCE = (node: string) => `#!${node}
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { randomUUID } = require("node:crypto");

const dir = process.env.FAKE_HERDR_DIR;
let args = process.argv.slice(2);
if (args[0] === "--session") args = args.slice(2);
fs.mkdirSync(path.join(dir, "sessions"), { recursive: true });
fs.appendFileSync(path.join(dir, "calls.jsonl"), JSON.stringify(args) + "\\n");
const stateFile = path.join(dir, "state.json");
const state = fs.existsSync(stateFile) ? JSON.parse(fs.readFileSync(stateFile, "utf8")) : { workspaces: {}, panes: {}, agents: {}, n: 0 };
const save = () => fs.writeFileSync(stateFile, JSON.stringify(state));
const scenFile = path.join(dir, "scenario.json");
const scenario = () => JSON.parse(fs.readFileSync(scenFile, "utf8"));
const forAttempt = (sc, id) => (sc[id] ??= { worker: [], reviewer: [] });
const ok = (result) => { console.log(JSON.stringify({ id: "fake", result })); process.exit(0); };
const fail = (code, message) => { console.log(JSON.stringify({ id: "fake", error: { code, message } })); process.exit(1); };
const opt = (name) => { const i = args.indexOf(name); return i === -1 ? undefined : args[i + 1]; };
const iso = () => new Date().toISOString();
const append = (file, entry) => fs.appendFileSync(file, JSON.stringify(entry) + "\\n");
const assistant = (file, text, stopReason = "stop") =>
  append(file, { type: "message", id: randomUUID().slice(0, 8), timestamp: iso(), message: { role: "assistant", content: [{ type: "text", text }], provider: "github-copilot", model: "claude-haiku-4.5", usage: { input: 10, output: 20, cacheRead: 5, cacheWrite: 1, totalTokens: 36, cost: { total: 0.001 } }, stopReason, timestamp: Date.now() } });
const agentView = (name) => { const a = state.agents[name]; return { name, pane_id: a.pane, agent_status: "idle", agent_session: { kind: "path", value: a.session } }; };

const [group, cmd] = args;
if (group === "worktree" && cmd === "open") {
  const p = opt("--path");
  const id = "ws" + (++state.n);
  const existing = Object.entries(state.workspaces).find(([, w]) => w.path === p);
  if (existing) ok({ already_open: true, workspace: { workspace_id: existing[0] } });
  state.workspaces[id] = { path: p };
  state.panes["pw" + state.n] = { workspace: id, cwd: p };
  save();
  ok({ already_open: false, workspace: { workspace_id: id } });
}
if (group === "pane" && cmd === "list") {
  const ws = opt("--workspace");
  const panes = Object.entries(state.panes).filter(([, p]) => p.workspace === ws).map(([id, p]) => ({ pane_id: id, workspace_id: ws }));
  ok({ panes });
}
if (group === "pane" && cmd === "split") {
  const id = "ps" + (++state.n);
  state.panes[id] = { workspace: "ws0", cwd: opt("--cwd") };
  save();
  ok({ pane: { pane_id: id, workspace_id: "ws0" } });
}
if (group === "pane" && cmd === "close") ok({ type: "ok" });
if (group === "workspace" && cmd === "close") ok({ type: "ok" });
if (group === "agent" && cmd === "start") {
  const name = args[2];
  const pane = opt("--pane");
  const sep = args.indexOf("--");
  const agentArgs = sep === -1 ? [] : args.slice(sep + 1);
  const role = agentArgs.includes("--tools") ? "reviewer" : "worker";
  const cwd = state.panes[pane].cwd;
  // A Worker's cwd is its Attempt worktree, which wt names after the branch (...--<attempt id>).
  const attempt = role === "worker" ? /--([a-z][a-z0-9-]*-a[0-9]+)$/.exec(cwd)[1] : null;
  if (role === "worker" && forAttempt(scenario(), attempt).startFails) fail("agent_not_ready", "fake: the agent did not become ready");
  const session = path.join(dir, "sessions", name + ".jsonl");
  append(session, { type: "session", version: 3, id: randomUUID(), timestamp: iso(), cwd, ...(agentArgs.includes("--fork") ? { parentSession: agentArgs[agentArgs.indexOf("--fork") + 1] } : {}) });
  state.agents[name] = { pane, session, role, cwd, attempt, gone: false, args: agentArgs };
  save();
  ok({ agent: agentView(name), argv: ["pi", ...agentArgs] });
}
if (group === "agent" && cmd === "get") {
  const name = args[2];
  if (!state.agents[name] || state.agents[name].gone) fail("agent_not_found", "fake: no such agent " + name);
  ok({ agent: agentView(name) });
}
if (group === "agent" && cmd === "prompt") {
  const name = args[2];
  const text = args[3];
  const a = state.agents[name];
  if (!a || a.gone) fail("agent_not_found", "fake: no such agent " + name);
  if (!a.attempt) a.attempt = /reviews\\/([^/]+)\\.md/.exec(text)[1];
  const sc = scenario();
  const queue = forAttempt(sc, a.attempt)[a.role];
  const action = queue.shift() ?? (a.role === "worker" ? "idle" : "garbage");
  fs.writeFileSync(scenFile, JSON.stringify(sc));
  if (a.role === "worker") {
    if (action === "crash") {
      a.gone = true;
      assistant(a.session, "working...", "toolUse");
    } else if (action === "idle") {
      assistant(a.session, "I am done thinking.");
    } else if (action === "submit") {
      const id = /(?:attempt id is |cpd attempt submit )([a-z][a-z0-9-]*-a[0-9]+)/.exec(text)[1];
      fs.writeFileSync(path.join(a.cwd, "feature-" + id + ".txt"), "made by " + name + "\\n");
      const git = (...g) => { const r = spawnSync("git", ["-C", a.cwd, ...g], { encoding: "utf8" }); if (r.status !== 0) throw new Error(r.stderr); };
      git("add", "-A");
      git("-c", "user.name=Worker", "-c", "user.email=w@example.com", "commit", "-q", "-m", "feat: " + id);
      const r = spawnSync(process.execPath, [process.env.FAKE_CPD, "attempt", "submit", id, "--wf", process.env.FAKE_WF], { cwd: a.cwd, encoding: "utf8", env: process.env });
      assistant(a.session, "submit said: " + r.stdout.trim());
    }
  } else {
    const verdict = action === "approve" ? { verdict: "approve", feedback: "fake reviewer: all acceptance items met" } : action === "reject" ? { verdict: "reject", feedback: "fake reviewer: please fix the thing" } : null;
    assistant(a.session, verdict ? "Reviewed.\\n\\n\`\`\`json\\n" + JSON.stringify(verdict) + "\\n\`\`\`" : "I looked at it and it seems fine.");
  }
  save();
  ok({ agent: agentView(name) });
}
fail("unsupported", "fake herdr does not know: " + args.join(" "));
`;

export type Scenario = Record<string, { worker: string[]; reviewer: string[]; startFails?: boolean }>;

export function installFakeHerdr(binDir: string, stateDir: string, scenario: Scenario): void {
  mkdirSync(binDir, { recursive: true });
  mkdirSync(stateDir, { recursive: true });
  const path = join(binDir, "herdr");
  writeFileSync(path, SOURCE(process.execPath));
  chmodSync(path, 0o755);
  setScenario(stateDir, scenario);
}

export function setScenario(stateDir: string, scenario: Scenario): void {
  writeFileSync(join(stateDir, "scenario.json"), JSON.stringify(scenario));
}

export function fakeHerdrCalls(stateDir: string): string[][] {
  try {
    return readFileSync(join(stateDir, "calls.jsonl"), "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l) as string[]);
  } catch {
    return [];
  }
}

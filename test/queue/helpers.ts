import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { sandboxEnv } from "../helpers/index.ts";

export const WF = "pay-4521";

/** Sandbox with a Workflow; `attached` controls whether a Lead is attached. Sets process.env.CPD_HOME. */
export function setup(attached = true) {
  const env = sandboxEnv();
  process.env.CPD_HOME = env.CPD_HOME!;
  const home = env.CPD_HOME!;
  const wfDir = join(home, "workflows", WF);
  mkdirSync(wfDir, { recursive: true });
  const queueDir = join(home, "leads", "sess1", "queue");
  const lead = attached
    ? { paneId: "w1:p1", sessionPath: "/x/s.jsonl", sessionId: "sess1", queueDir, attachedAt: new Date().toISOString() }
    : null;
  writeFileSync(
    join(wfDir, "workflow.json"),
    JSON.stringify({ v: 1, id: WF, title: "t", repo: "/r", base: "main", leadBranch: `cpd/${WF}`, leadWorktree: null, createdAt: "x", lead, pr: null }),
  );
  writeFileSync(join(home, "config.json"), JSON.stringify({
    version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5, prPollSeconds: 120,
    herdrSession: null, tiers: { O: "p/o", S: "p/s", H: "p/h" }, leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
  }));
  return { env, home, queueDir, escDir: join(wfDir, "escalations") };
}

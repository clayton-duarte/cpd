import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { planLock } from "../../src/dispatch/plan-lock.ts";
import { gitOut } from "../../src/git/git.ts";
import { initWorkflow, readWorkflow, setPhase, writeWorkflow } from "../../src/ledger/workflow.ts";
import { cpdHome } from "../../src/lib/paths.ts";
import { baseConfig } from "../herdr/setup.ts";
import { fakeBin, tmpDir, type FakeBin } from "../helpers/index.ts";
export { withFakeWt } from "../helpers/index.ts";
import { createRepo, sandbox } from "../git/fixtures.ts";
import { addDraft, approvePlan, mergeJob } from "../ledger/helpers.ts";

export const SLUG = "acme/widgets";
export const PR_URL = `https://github.com/${SLUG}/pull/7`;

/** Sandboxed CPD_HOME (with config) for this test process. Call once at the top of a test file. */
export function sandboxCpd(): void {
  sandbox();
  mkdirSync(cpdHome(), { recursive: true });
  writeFileSync(join(cpdHome(), "config.json"), JSON.stringify({ ...baseConfig, herdrSession: "fake-session" }));
}

/**
 * A fake `gh` on PATH. It answers from files in `dir` (`view.json`, `comments.json`), so a test can change
 * what the "server" says between calls. Every call's argv is logged.
 */
export function fakeGh(): FakeBin & { set(file: "view" | "comments", value: unknown): void } {
  const dir = tmpDir("cpd-fake-gh-data-");
  const gh = fakeBin(
    "gh",
    `case "$1 $2" in
  "repo view") echo '{"nameWithOwner":"${SLUG}"}' ;;
  "pr create") echo 'Creating pull request for cpd/demo into main'; echo '${PR_URL}' ;;
  "pr view") cat '${dir}/view.json' ;;
  "api "*) cat '${dir}/comments.json' ;;
  *) echo "fake gh: unexpected $*" >&2; exit 9 ;;
esac`,
  );
  process.env.PATH = `${gh.dir}:${process.env.PATH}`;
  const set = (file: "view" | "comments", value: unknown) => writeFileSync(join(dir, `${file}.json`), JSON.stringify(value));
  set("view", viewJson());
  set("comments", []);
  chmodSync(gh.path, 0o755);
  return Object.assign(gh, { set });
}

export const viewJson = (over: Record<string, unknown> = {}) => ({
  state: "OPEN",
  mergedAt: null,
  statusCheckRollup: [],
  reviews: [],
  comments: [],
  ...over,
});

export type PrFixture = { wf: string; repo: string; origin: string; leadWorktree: string; queueDir: string };

/**
 * A Workflow in phase `dispatch` whose Jobs are all Merged, a bare repo as `origin`, a Lead worktree,
 * and an attached Lead (a queue dir). `jobs` are merged; `extra` Jobs are added and left `queued`.
 */
export async function dispatchedWorkflow(wf: string, opts: { jobs?: string[]; extra?: string[] } = {}): Promise<PrFixture> {
  const repo = await createRepo();
  const origin = join(tmpDir("cpd-origin-"), "origin.git");
  await gitOut(repo, ["init", "-q", "--bare", origin]);
  await gitOut(repo, ["remote", "add", "origin", origin]);
  await gitOut(repo, ["push", "-q", "origin", "main"]);
  await initWorkflow({ wf, repo, title: `Workflow ${wf}`, base: "main" });
  await setPhase(wf, "plan");
  const merged = opts.jobs ?? ["a", "b"];
  for (const k of [...merged, ...(opts.extra ?? [])]) await addDraft(wf, k);
  await approvePlan(wf);
  const locked = await planLock(wf, { watch: false });
  for (const k of merged) await mergeJob(wf, k);
  const queueDir = join(tmpDir("cpd-queue-"), "queue");
  mkdirSync(queueDir, { recursive: true });
  await writeWorkflow(wf, {
    ...readWorkflow(wf),
    lead: { paneId: "w1:p1", sessionPath: "/sessions/lead.jsonl", sessionId: "lead1", queueDir, attachedAt: "2026-01-01T00:00:00.000Z" },
  });
  return { wf, repo, origin, leadWorktree: locked.leadWorktree, queueDir };
}

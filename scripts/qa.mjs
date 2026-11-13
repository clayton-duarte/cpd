#!/usr/bin/env node
// Dependency-free guard and creator for the single-operator QA workflow.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export const REPOS = ["clayton-duarte/cpd", "clayton-duarte/dotfiles"];
export const MARKER = "<!-- cpd-repo-qa:v1 -->";
export const ZONE = "America/Toronto";
export const batchMarker = (batch) => `<!-- cpd-repo-qa-batch:${batch} -->`;
const LIMITS = { issues: 20, prs: 10 };
const formatter = new Intl.DateTimeFormat("en-CA", {
  timeZone: ZONE, year: "numeric", month: "2-digit", day: "2-digit",
});
export function day(value) {
  const parts = Object.fromEntries(formatter.formatToParts(new Date(value)).map((p) => [p.type, p.value]));
  return `${parts.year}-${parts.month}-${parts.day}`;
}

export function github(args, input) {
  // No shell interpolation, no automatic retries of writes, no token logging.
  return JSON.parse(execFileSync("gh", ["api", "--hostname", "github.com", ...args], {
    input: input === undefined ? undefined : JSON.stringify(input),
    encoding: "utf8", timeout: 30_000, maxBuffer: 16 * 1024 * 1024,
    stdio: ["pipe", "pipe", "pipe"],
  }));
}

export function status(api = github, now = new Date(), batch = process.env.CPD_QA_BATCH ?? null) {
  const today = day(now);
  if (batch !== null && (!/^\d{4}-\d{2}-\d{2}-[a-z0-9][a-z0-9-]{0,39}$/.test(batch) || !batch.startsWith(`${today}-`))) {
    throw new Error("CPD_QA_BATCH must be a dated identifier for today's Toronto date, e.g. YYYY-MM-DD-reset-2");
  }
  // Toronto days are at most 25 hours. REST's `since` filters updated_at,
  // so this includes every creation today; filter created_at locally.
  const since = new Date(now.getTime() - 26 * 3600_000).toISOString();
  const counts = { issues: 0, prs: 0 };
  const dayCounts = { issues: 0, prs: 0 };
  for (const repo of REPOS) {
    const seen = new Set();
    for (let page = 1; ; page++) {
      const items = api([`repos/${repo}/issues?state=all&since=${encodeURIComponent(since)}&per_page=100&page=${page}`]);
      if (!Array.isArray(items)) throw new Error(`Invalid pagination response for ${repo}`);
      const previousSize = seen.size;
      for (const item of items) {
        if (!item || !Number.isInteger(item.number) || typeof item.created_at !== "string"
            || !Number.isFinite(Date.parse(item.created_at))
            || !(item.body === null || typeof item.body === "string")) {
          throw new Error(`Invalid issue response for ${repo}`);
        }
        if (seen.has(item.number)) continue;
        seen.add(item.number);
        if (item.body?.includes(MARKER) && day(item.created_at) === today) {
          const kind = item.pull_request ? "prs" : "issues";
          dayCounts[kind]++;
          if (batch === null || item.body.includes(batchMarker(batch))) counts[kind]++;
        }
      }
      if (items.length < 100) break;
      if (seen.size === previousSize) throw new Error(`Pagination made no progress for ${repo}`);
    }
  }
  const remaining = Object.fromEntries(Object.entries(LIMITS).map(([key, limit]) => [key, Math.max(0, limit - counts[key])]));
  return { day: today, timezone: ZONE, batch, repositories: REPOS, counts, day_counts: dayCounts, limits: LIMITS, remaining,
    can_create_issue: remaining.issues > 0, can_create_pr: remaining.prs > 0 };
}

export function run(args, api = github, read = (path) => readFileSync(path, "utf8"), batch = process.env.CPD_QA_BATCH ?? null) {
  const [command, repo, ...rest] = args;
  if (command === "status" && args.length === 1) return { exit: 0, result: status(api, new Date(), batch) };
  if (!["issue", "pr"].includes(command) || !REPOS.includes(repo)
      || rest.length !== (command === "issue" ? 2 : 3) || rest.some((s) => !s.trim())) {
    throw new Error("Usage: node scripts/qa.mjs status | issue OWNER/REPO TITLE BODY_FILE | pr OWNER/REPO HEAD TITLE BODY_FILE (base: main)");
  }
  const [head, title, file] = command === "pr" ? rest : [undefined, ...rest];
  const body = read(file);
  if (!body.trim()) throw new Error("Body file must not be empty");
  const budget = status(api, new Date(), batch);
  if (!(command === "issue" ? budget.can_create_issue : budget.can_create_pr)) {
    return { exit: 3, result: { ...budget, refused: command } };
  }
  const payload = { title, body: `${MARKER}\n${batch === null ? "" : `${batchMarker(batch)}\n`}\n${body}` };
  if (command === "pr") Object.assign(payload, { head, base: "main", draft: false });
  const result = api([`repos/${repo}/${command === "pr" ? "pulls" : "issues"}`, "--method", "POST", "--input", "-"], payload);
  return { exit: 0, result: { number: result.number, url: result.html_url, repository: repo, kind: command } };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const { exit, result } = run(process.argv.slice(2));
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    process.exitCode = exit;
  } catch (error) {
    // A failed write may have succeeded remotely. Inspect GitHub before retrying.
    process.stderr.write(`${JSON.stringify({ error: error.message, creation_allowed: false })}\n`);
    process.exitCode = 1;
  }
}

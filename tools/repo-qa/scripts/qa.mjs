#!/usr/bin/env node
// GitHub is the creation ledger. Config and optional run files contain intent only.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const object = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
const fields = (value, names, what) => {
  if (!object(value) || Object.keys(value).some((key) => !names.includes(key))) throw new Error(`Invalid ${what} fields`);
};
const limits = (value) => {
  fields(value, ["issues", "prs"], "limits");
  if (![value.issues, value.prs].every((n) => Number.isSafeInteger(n) && n >= 0)) throw new Error("Limits must be nonnegative integers");
  return Object.freeze({ issues: value.issues, prs: value.prs });
};
const nonempty = (v) => typeof v === "string" && v.trim() !== "";
const id = (v) => typeof v === "string" && /^[a-z0-9][a-z0-9-]{0,63}$/.test(v);
const remaining = (caps, counts) => caps === null ? null : Object.fromEntries(Object.entries(caps).map(([k, n]) => [k, Math.max(0, n - counts[k])]));

export function validateWeeklyPolicy(value) {
  fields(value, ["min_remaining_percent", "max_age_seconds"], "weekly allowance policy");
  if (!Number.isFinite(value.min_remaining_percent) || value.min_remaining_percent <= 0 || value.min_remaining_percent > 100
      || !Number.isSafeInteger(value.max_age_seconds) || value.max_age_seconds < 1 || value.max_age_seconds > 60) {
    throw new Error("Weekly policy needs a threshold in (0,100] and freshness of 1..60 seconds");
  }
  return Object.freeze({ ...value });
}

export function readWeeklyUsage(policy, now = new Date(), read = () => {
  if (!process.env.QA_USAGE_FILE) throw new Error("QA_USAGE_FILE must name the live reader's snapshot");
  return JSON.parse(readFileSync(process.env.QA_USAGE_FILE, "utf8"));
}) {
  const value = read();
  fields(value, ["version", "source", "sampled_at", "limit_id", "window_minutes", "remaining_percent", "resets_at",
    "ordinary_usage_allowed", "spend_control_reached"], "weekly usage");
  const sampled = typeof value.sampled_at === "string" ? Date.parse(value.sampled_at) : NaN;
  const reset = typeof value.resets_at === "string" ? Date.parse(value.resets_at) : NaN;
  if (value.version !== 1 || value.source !== "codex-app-server" || value.limit_id !== "codex" || value.window_minutes !== 10080
      || !Number.isFinite(value.remaining_percent) || value.remaining_percent < 0 || value.remaining_percent > 100
      || !Number.isFinite(sampled) || sampled > now.getTime() || now.getTime() - sampled > policy.max_age_seconds * 1000
      || !Number.isFinite(reset) || reset <= now.getTime() || reset - sampled > 10080 * 60_000
      || typeof value.ordinary_usage_allowed !== "boolean" || typeof value.spend_control_reached !== "boolean") {
    throw new Error("Weekly allowance is missing, invalid, stale or reset; refresh the authenticated reader");
  }
  return { ...value, allowed: value.remaining_percent >= policy.min_remaining_percent
    && value.ordinary_usage_allowed && !value.spend_control_reached };
}

export function validateConfig(value) {
  fields(value, ["version", "scope", "timezone", "limits", "repositories", "weekly_allowance"], "config");
  if (value.version !== 1 || !id(value.scope) || !nonempty(value.timezone)) throw new Error("Config needs version 1, a scope identifier and an IANA timezone");
  new Intl.DateTimeFormat("en-CA", { timeZone: value.timezone });
  if (!Array.isArray(value.repositories) || value.repositories.length === 0) throw new Error("Config needs a repository allowlist");
  const repos = value.repositories.map((repo) => {
    fields(repo, ["name", "base"], "repository");
    if (typeof repo.name !== "string" || !/^[A-Za-z0-9][A-Za-z0-9-]*\/[A-Za-z0-9_.-]+$/.test(repo.name)
        || !nonempty(repo.base) || repo.base.startsWith("-") || /\s/.test(repo.base)) throw new Error("Repository needs OWNER/REPO and an explicit base branch");
    return Object.freeze({ name: repo.name, base: repo.base });
  });
  if (new Set(repos.map((r) => r.name.toLowerCase())).size !== repos.length) throw new Error("Duplicate repository in config");
  const weekly = value.weekly_allowance === undefined ? null : validateWeeklyPolicy(value.weekly_allowance);
  if (value.limits === null && weekly === null) throw new Error("Null cardinal limits require a weekly allowance policy");
  return Object.freeze({ weekly_allowance: weekly, version: 1, scope: value.scope, timezone: value.timezone, limits: value.limits === null ? null : limits(value.limits), repositories: Object.freeze(repos) });
}

export function validateRun(value, now = new Date()) {
  fields(value, ["version", "id", "started_at", "limits", "authorization"], "run");
  if (value.version !== 1 || !id(value.id) || !nonempty(value.authorization)
      || typeof value.started_at !== "string" || !Number.isFinite(Date.parse(value.started_at))
      || Date.parse(value.started_at) > now.getTime()) throw new Error("Run needs version 1, id, past/current started_at and authorization reference");
  return Object.freeze({ ...value, limits: value.limits === null ? null : limits(value.limits) });
}

export function github(args, input) {
  return JSON.parse(execFileSync("gh", ["api", "--hostname", "github.com", ...args], {
    input: input === undefined ? undefined : JSON.stringify(input),
    encoding: "utf8", timeout: 30_000, maxBuffer: 16 * 1024 * 1024,
    stdio: ["pipe", "pipe", "pipe"],
  }));
}

export function createGuard(input, { readUsage, clock = () => new Date() } = {}) {
  const config = validateConfig(input);
  const REPOS = config.repositories.map((r) => r.name);
  const MARKER = `<!-- ${config.scope}:v1 -->`;
  const batchMarker = (batch) => `<!-- ${config.scope}-batch:${batch} -->`;
  const runMarker = (run) => `<!-- ${config.scope}-run:${run} -->`;
  const formatter = new Intl.DateTimeFormat("en-CA", { timeZone: config.timezone, year: "numeric", month: "2-digit", day: "2-digit" });
  const day = (value) => {
    const parts = Object.fromEntries(formatter.formatToParts(new Date(value)).map((p) => [p.type, p.value]));
    return `${parts.year}-${parts.month}-${parts.day}`;
  };

  function status(api = github, now = clock(), batch = null, runInput = null) {
    let weekly = config.weekly_allowance === null ? null : readWeeklyUsage(config.weekly_allowance, now, readUsage);
    const today = day(now);
    if (batch !== null && (typeof batch !== "string" || !/^\d{4}-\d{2}-\d{2}-[a-z0-9][a-z0-9-]{0,39}$/.test(batch) || !batch.startsWith(`${today}-`))) {
      throw new Error("QA_BATCH must be a dated batch identifier for today's configured timezone");
    }
    const run = runInput === null ? null : validateRun(runInput, now);
    // A conservative two-day day lookback; long runs extend it to their start.
    // Floor before subtracting a second because GitHub creation timestamps have second precision.
    const runStart = run === null ? Infinity : Math.floor(Date.parse(run.started_at) / 1000) * 1000 - 1000;
    const since = new Date(Math.min(now.getTime() - 48 * 3600_000, runStart)).toISOString();
    const counts = { issues: 0, prs: 0 };
    const dayCounts = { issues: 0, prs: 0 };
    const runCounts = { issues: 0, prs: 0 };
    for (const repo of REPOS) {
      const seen = new Set();
      for (let page = 1; ; page++) {
        const items = api([`repos/${repo}/issues?state=all&since=${encodeURIComponent(since)}&per_page=100&page=${page}`]);
        if (!Array.isArray(items)) throw new Error(`Invalid pagination response for ${repo}`);
        const previousSize = seen.size;
        for (const item of items) {
          if (!item || !Number.isInteger(item.number) || typeof item.created_at !== "string"
              || !Number.isFinite(Date.parse(item.created_at)) || !(item.body === null || typeof item.body === "string")) {
            throw new Error(`Invalid issue response for ${repo}`);
          }
          if (seen.has(item.number)) continue;
          seen.add(item.number);
          if (!item.body?.includes(MARKER)) continue;
          const kind = item.pull_request ? "prs" : "issues";
          if (run !== null && item.body.includes(runMarker(run.id))) runCounts[kind]++;
          if (day(item.created_at) === today) {
            dayCounts[kind]++;
            if (batch === null || item.body.includes(batchMarker(batch))) counts[kind]++;
          }
        }
        if (items.length < 100) break;
        if (seen.size === previousSize) throw new Error(`Pagination made no progress for ${repo}`);
      }
    }
    if (weekly !== null) {
      const fresh = readWeeklyUsage(config.weekly_allowance, clock(), readUsage);
      if (Math.abs(Date.parse(fresh.resets_at) - Date.parse(weekly.resets_at)) > 1000) throw new Error("Weekly window changed; explicit authorization is required");
      weekly = fresh;
    }
    const dailyRemaining = remaining(config.limits, counts);
    const runRemaining = run === null ? null : remaining(run.limits, runCounts);
    const effective = Object.fromEntries(["issues", "prs"].map((k) => {
      const n = Math.min(dailyRemaining?.[k] ?? Infinity, runRemaining?.[k] ?? Infinity);
      return [k, n === Infinity ? null : n];
    }));
    return { scope: config.scope, day: today, timezone: config.timezone, batch, repositories: REPOS,
      weekly_allowance: weekly, counts, day_counts: dayCounts, limits: config.limits, daily_remaining: dailyRemaining,
      run: run === null ? null : { ...run, counts: runCounts, remaining: runRemaining }, remaining: effective,
      can_create_issue: (weekly?.allowed ?? true) && (effective.issues === null || effective.issues > 0),
      can_create_pr: (weekly?.allowed ?? true) && (effective.prs === null || effective.prs > 0) };
  }

  function prStatus(api = github, batch = null, descriptor = null) {
    const budget = status(api, clock(), batch, descriptor);
    if (budget.weekly_allowance?.allowed === false) return { exit: 3, result: { ...budget, refused: "pr-status" } };
    const pullRequests = [];
    for (const repo of REPOS) {
      const seen = new Set();
      for (let page = 1; ; page++) {
        const items = api([`repos/${repo}/pulls?state=open&per_page=100&page=${page}`]);
        if (!Array.isArray(items)) throw new Error(`Invalid open PR response for ${repo}`);
        const previousSize = seen.size;
        for (const item of items) {
          if (!item || !Number.isInteger(item.number) || item.number <= 0 || !(item.body === null || typeof item.body === "string")) {
            throw new Error(`Invalid open PR response for ${repo}`);
          }
          if (seen.has(item.number)) continue;
          seen.add(item.number);
          if (!item.body?.includes(MARKER)) continue;
          const info = api([`repos/${repo}/pulls/${item.number}`]);
          if (!info || info.number !== item.number || !["open", "closed"].includes(info.state)
              || !(info.body === null || typeof info.body === "string")) throw new Error(`Invalid PR metadata for ${repo}#${item.number}`);
          if (info.state === "closed" || !info.body?.includes(MARKER)) continue;
          if (![true, false, null].includes(info.mergeable) || !/^[0-9a-f]{40}$/i.test(info.head?.sha ?? "")
              || !/^[0-9a-f]{40}$/i.test(info.base?.sha ?? "") || !nonempty(info.head?.ref) || !nonempty(info.base?.ref)) {
            throw new Error(`Unverifiable mergeability for ${repo}#${item.number}`);
          }
          pullRequests.push({ repository: repo, number: info.number, url: info.html_url ?? `https://github.com/${repo}/pull/${info.number}`,
            head_sha: info.head.sha, base_sha: info.base.sha, head_branch: info.head.ref, base_branch: info.base.ref,
            mergeable: info.mergeable, mergeability: info.mergeable === null ? "pending" : info.mergeable ? "mergeable" : "conflicted" });
        }
        if (items.length < 100) break;
        if (seen.size === previousSize) throw new Error(`Open PR pagination made no progress for ${repo}`);
      }
    }
    if (budget.weekly_allowance !== null) {
      const fresh = readWeeklyUsage(config.weekly_allowance, clock(), readUsage);
      if (Math.abs(Date.parse(fresh.resets_at) - Date.parse(budget.weekly_allowance.resets_at)) > 1000) throw new Error("Weekly window changed; explicit authorization is required");
      budget.weekly_allowance = fresh;
      budget.can_create_issue &&= fresh.allowed;
      budget.can_create_pr &&= fresh.allowed;
      if (!fresh.allowed) return { exit: 3, result: { ...budget, pull_requests: pullRequests, refused: "pr-status" } };
    }
    return { exit: pullRequests.some((pr) => !pr.mergeable) ? 4 : 0,
      result: { ...budget, checked_at: clock().toISOString(), pull_requests: pullRequests } };
  }

  function run(args, api = github, read = (file) => readFileSync(file, "utf8"), batch = null, descriptor = null) {
    const [command, repo, ...rest] = args;
    if (command === "status" && args.length === 1) return { exit: 0, result: status(api, clock(), batch, descriptor) };
    if (command === "pr-status" && args.length === 1) return prStatus(api, batch, descriptor);
    if (!["issue", "pr"].includes(command) || !REPOS.includes(repo)
        || rest.length !== (command === "issue" ? 2 : 3) || rest.some((s) => !nonempty(s))) {
      throw new Error("Usage: qa.mjs [--config FILE] [--run FILE] status | pr-status | issue OWNER/REPO TITLE BODY_FILE | pr OWNER/REPO HEAD TITLE BODY_FILE");
    }
    const [head, title, file] = command === "pr" ? rest : [undefined, ...rest];
    const body = read(file);
    if (!nonempty(body)) throw new Error("Body file must not be empty");
    const budget = status(api, clock(), batch, descriptor);
    if (!(command === "issue" ? budget.can_create_issue : budget.can_create_pr)) return { exit: 3, result: { ...budget, refused: command } };
    if (config.weekly_allowance !== null) {
      const fresh = readWeeklyUsage(config.weekly_allowance, clock(), readUsage);
      if (Math.abs(Date.parse(fresh.resets_at) - Date.parse(budget.weekly_allowance.resets_at)) > 1000) throw new Error("Weekly window changed; explicit authorization is required");
      if (!fresh.allowed) return { exit: 3, result: { refused: command, reason: "weekly allowance" } };
    }
    const markers = [MARKER, ...(batch === null ? [] : [batchMarker(batch)]), ...(descriptor === null ? [] : [runMarker(descriptor.id)])];
    const payload = { title, body: `${markers.join("\n")}\n\n${body}` };
    if (command === "pr") Object.assign(payload, { head, base: config.repositories.find((r) => r.name === repo).base, draft: false });
    const result = api([`repos/${repo}/${command === "pr" ? "pulls" : "issues"}`, "--method", "POST", "--input", "-"], payload);
    return { exit: 0, result: { number: result.number, url: result.html_url, repository: repo, kind: command } };
  }
  return { config, REPOS, MARKER, batchMarker, runMarker, day, status, prStatus, run };
}

export function cli(args, { configFile = "qa.config.json", batch = process.env.QA_BATCH ?? null } = {}) {
  try {
    let runFile = null;
    const seen = new Set();
    while (args[0]?.startsWith("--")) {
      const flag = args.shift();
      if (!["--config", "--run"].includes(flag) || seen.has(flag) || !nonempty(args[0]) || args[0].startsWith("--")) throw new Error("Expected unique --config FILE or --run FILE before the command");
      seen.add(flag);
      const file = args.shift();
      if (flag === "--config") configFile = file;
      else runFile = file;
    }
    const guard = createGuard(JSON.parse(readFileSync(configFile, "utf8")));
    const descriptor = runFile === null ? null : validateRun(JSON.parse(readFileSync(runFile, "utf8")));
    const { exit, result } = guard.run(args, github, undefined, batch, descriptor);
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    process.exitCode = exit;
  } catch (error) {
    process.stderr.write(`${JSON.stringify({ error: error.message, creation_allowed: false })}\n`);
    process.exitCode = 1;
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) cli(process.argv.slice(2));

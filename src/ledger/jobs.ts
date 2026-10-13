import { readFileSync } from "node:fs";
import { z } from "zod";
import { failure, notFound, refused, usage } from "../lib/errors.ts";
import { assertJobKey } from "../lib/paths.ts";
import { loadConfig } from "../lib/config.ts";
import { formatTier, loadCatalog, parseTier, validTier, type Catalog } from "../lib/tiers.ts";
import { logEvent } from "./audit.ts";
import { createIssue, listIssues, metadataArgs, readLedger, withLedger, workflowIssue, type ReadLedger } from "./bd.ts";
import { fingerprintOfIssue, keyMap, needsOf, writeFingerprints } from "./fingerprint.ts";
import { BdComments, BdIssues, type BdIssue } from "./schema.ts";
import { getPhaseIn } from "./workflow.ts";

// Contract §8 (Job mapping) and §11 (`job *`).

export const JOB_STATUSES = ["draft", "queued", "running", "in_review", "closed", "failed", "cancelled"] as const;
export type JobStatus = (typeof JOB_STATUSES)[number];
const JobStatus = z.enum(JOB_STATUSES);

export const JobSpecFile = z.strictObject({
  goal: z.string().min(1),
  context: z.string(),
  touches: z.array(z.string().min(1)).min(1),
  acceptance: z.array(z.string().min(1)).min(1),
  outOfScope: z.array(z.string()),
});
export type JobSpec = z.infer<typeof JobSpecFile>;

/** Read and validate a `--spec` file (contract §11). */
export function readSpecFile(path: string): JobSpec {
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    throw usage("spec_unreadable", `cannot read spec file ${path}: ${(e as Error).message}`, { path });
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw usage("spec_invalid", `spec file is not valid JSON: ${path}`, { path });
  }
  return parseSpec(raw, path);
}

/** Validate a Spec object; `spec_invalid` (exit 2) with the zod issues. */
export function parseSpec(raw: unknown, source = "spec"): JobSpec {
  const parsed = JobSpecFile.safeParse(raw);
  if (!parsed.success) throw usage("spec_invalid", `${source} does not match the Spec schema`, { source, issues: parsed.error.issues });
  return parsed.data;
}

const JobMeta = z.looseObject({
  key: z.string(),
  tier: z.string(),
  review_tier: z.string(),
  context: z.string(),
  touches: z.array(z.string()),
  acceptance: z.array(z.string()),
  out_of_scope: z.array(z.string()),
});

export type Job = {
  /** beads issue id */
  id: string;
  key: string;
  title: string;
  status: JobStatus;
  tier: string;
  reviewTier: string;
  goal: string;
  context: string;
  touches: string[];
  acceptance: string[];
  outOfScope: string[];
  /** Keys of the Jobs this one needs. */
  needs: string[];
  closeReason: string | null;
  metadata: Record<string, unknown>;
};

function toJob(issue: BdIssue, keys: Map<string, string>): Job {
  const status = JobStatus.safeParse(issue.status);
  const meta = JobMeta.safeParse(issue.metadata);
  if (!status.success || !meta.success) {
    throw failure("ledger_invalid", `Job issue ${issue.id} does not have the expected shape`, {
      id: issue.id,
      issues: [...(status.success ? [] : status.error.issues), ...(meta.success ? [] : meta.error.issues)],
    });
  }
  const m = meta.data;
  return {
    id: issue.id,
    key: m.key,
    title: issue.title,
    status: status.data,
    tier: m.tier,
    reviewTier: m.review_tier,
    goal: issue.description ?? "",
    context: m.context,
    touches: m.touches,
    acceptance: m.acceptance,
    outOfScope: m.out_of_scope,
    needs: needsOf(issue, keys),
    closeReason: issue.close_reason ?? null,
    metadata: m,
  };
}

/** `<prefix>.<n>`: the Job's creation order. */
function creationOrder(a: BdIssue, b: BdIssue): number {
  const n = (i: BdIssue) => Number(i.id.slice(i.id.lastIndexOf(".") + 1));
  return n(a) - n(b);
}

export type Loaded = { jobs: Job[]; issues: BdIssue[]; keys: Map<string, string> };

/** Every Job of the Workflow in creation order, plus the raw issues. */
export async function loadJobs(l: ReadLedger): Promise<Loaded> {
  const issues = (await listIssues(l, "job")).sort(creationOrder);
  const keys = keyMap(issues);
  return { jobs: issues.map((i) => toJob(i, keys)), issues, keys };
}

export const rawOf = (loaded: Loaded, job: Job): BdIssue => loaded.issues.find((i) => i.id === job.id)!;

function find(jobs: Job[], key: string): Job {
  const job = jobs.find((j) => j.key === key);
  if (!job) throw notFound("job_not_found", `no Job with key ${key}`, { key });
  return job;
}

export async function listJobs(wf: string, opts: { status?: string } = {}): Promise<Job[]> {
  let status: JobStatus | undefined;
  if (opts.status !== undefined) {
    const s = JobStatus.safeParse(opts.status);
    if (!s.success) throw usage("invalid_status", `invalid status ${JSON.stringify(opts.status)}: expected one of ${JOB_STATUSES.join(", ")}`);
    status = s.data;
  }
  const { jobs } = await loadJobs(readLedger(wf));
  return status ? jobs.filter((j) => j.status === status) : jobs;
}

export async function getJobByKey(wf: string, key: string): Promise<Job> {
  assertJobKey(key);
  return find((await loadJobs(readLedger(wf))).jobs, key);
}

/** `bd ready -t job -n 0` filtered to `queued`, in creation order. Does not verify fingerprints: Dispatch steps call `verifyJob`. */
export async function readyJobs(wf: string): Promise<Job[]> {
  const l = readLedger(wf);
  const ready = await l.read(["ready", "-t", "job", "-n", "0"], BdIssues);
  const { jobs } = await loadJobs(l);
  const ids = new Set(ready.filter((i) => i.status === "queued").map((i) => i.id));
  return jobs.filter((j) => ids.has(j.id));
}

// --- Attempts (read-only view; the Attempt lifecycle belongs to the ledger-attempts Job) ---

const AttemptMeta = z.looseObject({
  number: z.number().int().optional(),
  tier: z.string().optional(),
  outcome: z.string().optional(),
  cause: z.string().optional(),
});

export type AttemptSummary = { id: string; issue: string; number: number; tier: string | null; outcome: string | null; cause: string | null; status: string };

/** Attempts of every Job, by Job issue id, in Attempt-number order. */
async function loadAttempts(l: ReadLedger, loaded: Loaded): Promise<Map<string, AttemptSummary[]>> {
  const out = new Map<string, AttemptSummary[]>();
  for (const a of await listIssues(l, "attempt")) {
    const meta = AttemptMeta.safeParse(a.metadata ?? {});
    const job = a.parent === undefined ? undefined : loaded.jobs.find((j) => j.id === a.parent);
    if (!meta.success || !job || meta.data.number === undefined) {
      throw failure("ledger_invalid", `Attempt issue ${a.id} does not have the expected shape`, { id: a.id });
    }
    const list = out.get(job.id) ?? [];
    list.push({
      id: `${job.key}-a${meta.data.number}`,
      issue: a.id,
      number: meta.data.number,
      tier: meta.data.tier ?? null,
      outcome: meta.data.outcome ?? null,
      cause: meta.data.cause ?? null,
      status: a.status,
    });
    out.set(job.id, list);
  }
  for (const list of out.values()) list.sort((a, b) => a.number - b.number);
  return out;
}

/** The JSON shape commands print for a Job. */
export const jobView = (j: Job) => ({
  key: j.key,
  id: j.id,
  title: j.title,
  status: j.status,
  tier: j.tier,
  reviewTier: j.reviewTier,
  needs: j.needs,
});

export type JobWithAttempts = Job & { attempts: { id: string; tier: string | null; outcome: string | null }[] };

/** Every Job with its Attempts (`id`, `tier`, `outcome`), for `workflow show`. */
export async function listJobsWithAttempts(wf: string): Promise<JobWithAttempts[]> {
  const l = readLedger(wf);
  const loaded = await loadJobs(l);
  const attempts = await loadAttempts(l, loaded);
  return loaded.jobs.map((j) => ({
    ...j,
    attempts: (attempts.get(j.id) ?? []).map((a) => ({ id: a.id, tier: a.tier, outcome: a.outcome })),
  }));
}

const comments = async (l: ReadLedger, id: string) =>
  (await l.read(["comments", id], BdComments)).map((c) => ({ author: c.author, text: c.text, at: c.created_at }));

export type JobShow = {
  job: Job;
  comments: { author: string; text: string; at: string }[];
  attempts: (AttemptSummary & {
    comments: { author: string; text: string; at: string }[];
    escalations: { id: string; status: string; question: string; metadata: Record<string, unknown>; comments: { author: string; text: string; at: string }[] }[];
  })[];
};

/** A Job with its Attempts, their comments (verdict feedback) and their Escalations. */
export async function showJob(wf: string, key: string): Promise<JobShow> {
  assertJobKey(key);
  const l = readLedger(wf);
  const loaded = await loadJobs(l);
  const job = find(loaded.jobs, key);
  const attempts = (await loadAttempts(l, loaded)).get(job.id) ?? [];
  const escalations = await listIssues(l, "escalation");
  return {
    job,
    comments: await comments(l, job.id),
    attempts: await Promise.all(
      attempts.map(async (a) => ({
        ...a,
        comments: await comments(l, a.issue),
        escalations: await Promise.all(
          escalations
            .filter((e) => e.parent === a.issue)
            .map(async (e) => ({ id: e.id, status: e.status, question: e.description ?? "", metadata: e.metadata ?? {}, comments: await comments(l, e.id) })),
        ),
      })),
    ),
  };
}

// --- validation (no writes happen before all of this passes) ---

/** Comma-separated Job keys (`--needs`); the empty string is an empty list. */
export const splitKeys = (v: string): string[] => (v === "" ? [] : v.split(","));

/** Config and pi's Tier catalog, for commands that validate Tiers. */
export async function loadTierContext(): Promise<{ catalog: Catalog; defaultReviewTier: string }> {
  const config = loadConfig();
  return { catalog: await loadCatalog(config), defaultReviewTier: config.defaultReviewTier };
}

export function assertTier(catalog: Catalog, s: string): string {
  const t = parseTier(s);
  if (!validTier(catalog, t)) {
    throw usage("tier_invalid", `Tier ${formatTier(t)} is not valid: ${catalog[t.letter].model} supports efforts ${catalog[t.letter].efforts.join(",")}`, {
      tier: formatTier(t),
      efforts: catalog[t.letter].efforts,
    });
  }
  return formatTier(t);
}

/** A dependency cycle as a list of keys (first repeated at the end), or null. `graph` maps a key to the keys it needs. */
export function findCycle(graph: Map<string, string[]>): string[] | null {
  const state = new Map<string, 1 | 2>();
  const stack: string[] = [];
  const visit = (k: string): string[] | null => {
    if (state.get(k) === 2) return null;
    if (state.get(k) === 1) return [...stack.slice(stack.indexOf(k)), k];
    state.set(k, 1);
    stack.push(k);
    for (const n of graph.get(k) ?? []) {
      const c = visit(n);
      if (c) return c;
    }
    stack.pop();
    state.set(k, 2);
    return null;
  };
  for (const k of graph.keys()) {
    const c = visit(k);
    if (c) return c;
  }
  return null;
}

function assertNeeds(key: string, needs: string[], jobs: Job[]): void {
  if (new Set(needs).size !== needs.length) throw usage("needs_duplicate", "needs lists a Job twice", { needs });
  for (const n of needs) {
    assertJobKey(n);
    if (n === key) throw refused("dependency_cycle", `Job ${key} cannot need itself`, { cycle: [key, key] });
    const dep = jobs.find((j) => j.key === n);
    if (!dep) throw usage("needs_unknown", `needs references unknown Job ${n}`, { key: n });
    if (dep.status === "cancelled" || dep.status === "failed") {
      throw refused("needs_unavailable", `Job ${n} is ${dep.status} and cannot be a dependency`, { key: n, status: dep.status });
    }
  }
}

function assertAcyclic(jobs: Job[], key: string, needs: string[]): void {
  const graph = new Map(jobs.map((j) => [j.key, j.needs] as [string, string[]]));
  graph.set(key, needs);
  const cycle = findCycle(graph);
  if (cycle) throw refused("dependency_cycle", `dependency cycle: ${cycle.join(" -> ")}`, { cycle });
}

// --- writes ---

export type AddJobInput = { key: string; title: string; tier: string; reviewTier: string; spec: JobSpec; needs: string[] };

const specMetadata = (i: { key: string; tier: string; reviewTier: string; spec: JobSpec }) => ({
  key: i.key,
  tier: i.tier,
  review_tier: i.reviewTier,
  context: i.spec.context,
  touches: i.spec.touches,
  acceptance: i.spec.acceptance,
  out_of_scope: i.spec.outOfScope,
});

function assertTitle(title: string): void {
  if (title.trim() === "") throw usage("invalid_title", "title must not be empty");
}

/**
 * `job add`. Phase `plan`: a `draft`. Phase `dispatch`: an appended Job, fingerprinted and `queued`.
 * The Job is created as a draft and only flipped to `queued` after its Dependencies and fingerprint exist,
 * so a concurrent scheduler never sees it half-built.
 */
export async function addJob(wf: string, input: AddJobInput, catalog: Catalog): Promise<{ job: Job; fingerprint: string | null }> {
  assertJobKey(input.key);
  assertTitle(input.title);
  const tier = assertTier(catalog, input.tier);
  const reviewTier = assertTier(catalog, input.reviewTier);
  const spec = parseSpec(input.spec);
  return withLedger(wf, `job add ${input.key}`, async (l) => {
    const phase = await getPhaseIn(l);
    if (phase !== "plan" && phase !== "dispatch") {
      throw refused("illegal_phase", `jobs can only be added in phase plan or dispatch, not ${phase}`, { phase });
    }
    const loaded = await loadJobs(l);
    if (loaded.jobs.some((j) => j.key === input.key)) throw refused("job_key_exists", `a Job with key ${input.key} already exists`, { key: input.key });
    assertNeeds(input.key, input.needs, loaded.jobs);
    assertAcyclic(loaded.jobs, input.key, input.needs);

    const parent = await workflowIssue(l);
    const id = await createIssue(l, [
      `--title=${input.title}`,
      "-t",
      "job",
      "--parent",
      parent.id,
      "-s",
      "draft",
      `--description=${spec.goal}`,
      "--metadata",
      JSON.stringify(specMetadata({ key: input.key, tier, reviewTier, spec })),
    ]);
    for (const n of input.needs) await l.write(["dep", "add", id, find(loaded.jobs, n).id]);

    let fp: string | null = null;
    if (phase === "dispatch") {
      const after = await loadJobs(l);
      fp = fingerprintOfIssue(rawOf(after, find(after.jobs, input.key)), after.keys);
      writeFingerprints(wf, { [input.key]: fp });
      await l.write(["update", id, "--if-status", "draft", "-s", "queued"]);
      logEvent(wf, "info", "job_appended", { key: input.key, id, needs: input.needs, fingerprint: fp });
    }
    const final = await loadJobs(l);
    return { job: find(final.jobs, input.key), fingerprint: fp };
  });
}

export type EditJobPatch = { title?: string; tier?: string; reviewTier?: string; spec?: JobSpec; needs?: string[] };

/** `job edit`: draft only, else `job_locked` (exit 3). */
export async function editJob(wf: string, key: string, patch: EditJobPatch, catalog: Catalog): Promise<Job> {
  assertJobKey(key);
  if (Object.values(patch).every((v) => v === undefined)) throw usage("nothing_to_edit", "nothing to edit: pass at least one field");
  return withLedger(wf, `job edit ${key}`, async (l) => {
    const loaded = await loadJobs(l);
    const job = find(loaded.jobs, key);
    if (job.status !== "draft") throw refused("job_locked", `Job ${key} is ${job.status}; only draft Jobs can be edited`, { key, status: job.status });
    if (patch.title !== undefined) assertTitle(patch.title);
    const tier = patch.tier === undefined ? job.tier : assertTier(catalog, patch.tier);
    const reviewTier = patch.reviewTier === undefined ? job.reviewTier : assertTier(catalog, patch.reviewTier);
    const spec = patch.spec === undefined ? undefined : parseSpec(patch.spec);
    if (patch.needs !== undefined) {
      assertNeeds(key, patch.needs, loaded.jobs);
      assertAcyclic(loaded.jobs, key, patch.needs);
    }

    const effective = {
      key,
      tier,
      reviewTier,
      spec: spec ?? { goal: job.goal, context: job.context, touches: job.touches, acceptance: job.acceptance, outOfScope: job.outOfScope },
    };
    await l.write([
      "update",
      job.id,
      "--if-status",
      "draft",
      ...(patch.title === undefined ? [] : [`--title=${patch.title}`]),
      `--description=${effective.spec.goal}`,
      ...metadataArgs(job.metadata, specMetadata(effective)),
    ]);
    if (patch.needs !== undefined) {
      const want = new Set(patch.needs);
      for (const n of job.needs.filter((n) => !want.has(n))) await l.write(["dep", "remove", job.id, find(loaded.jobs, n).id]);
      for (const n of patch.needs.filter((n) => !job.needs.includes(n))) await l.write(["dep", "add", job.id, find(loaded.jobs, n).id]);
    }
    return find((await loadJobs(l)).jobs, key);
  });
}

const openAttempts = async (l: ReadLedger, job: Job) =>
  (await listIssues(l, "attempt", ["--parent", job.id])).filter((a) => a.status !== "closed");

/**
 * `job cancel`. Without an open Attempt the Job becomes `cancelled` at once. With one, the write-once
 * `cancel_requested` is recorded and `cpd watch` finishes the job. Dependents stay blocked either way.
 */
export async function cancelJob(wf: string, key: string, reason: string): Promise<{ job: Job; cancelled: boolean; alreadyRequested: boolean }> {
  assertJobKey(key);
  if (reason.trim() === "") throw usage("invalid_reason", "reason must not be empty");
  return withLedger(wf, `job cancel ${key}`, async (l) => {
    const loaded = await loadJobs(l);
    const job = find(loaded.jobs, key);
    if (job.status === "closed" || job.status === "cancelled") {
      throw refused("job_not_cancellable", `Job ${key} is ${job.status}`, { key, status: job.status });
    }
    const now = new Date().toISOString();
    let alreadyRequested = false;
    let cancelled = false;
    if ((await openAttempts(l, job)).length > 0) {
      if ("cancel_requested" in job.metadata) {
        alreadyRequested = true;
      } else {
        await l.write(["update", job.id, ...metadataArgs(job.metadata, { cancel_requested: now, cancel_reason: reason }, true)]);
      }
    } else {
      const patch = { cancelled_at: now, ...("cancel_reason" in job.metadata ? {} : { cancel_reason: reason }) };
      await l.write(["update", job.id, "--if-status", job.status, "-s", "cancelled", ...metadataArgs(job.metadata, patch, true)]);
      cancelled = true;
    }
    logEvent(wf, "info", "job_cancel", { key, cancelled, alreadyRequested, reason });
    return { job: find((await loadJobs(l)).jobs, key), cancelled, alreadyRequested };
  });
}

/** `job retry`: `failed` only. Records write-once `retry_<n>_tier` for the next Attempt number `n`, and queues the Job. */
export async function retryJob(wf: string, key: string, tier: string, catalog: Catalog): Promise<{ job: Job; attempt: number; tier: string }> {
  assertJobKey(key);
  const t = assertTier(catalog, tier);
  return withLedger(wf, `job retry ${key}`, async (l) => {
    const loaded = await loadJobs(l);
    const job = find(loaded.jobs, key);
    if (job.status !== "failed") throw refused("job_not_failed", `Job ${key} is ${job.status}; only failed Jobs can be retried`, { key, status: job.status });
    const attempt = (await listIssues(l, "attempt", ["--parent", job.id])).length + 1;
    await l.write([
      "update",
      job.id,
      "--if-status",
      "failed",
      "-s",
      "queued",
      ...metadataArgs(job.metadata, { [`retry_${attempt}_tier`]: t }, true),
    ]);
    logEvent(wf, "info", "job_retry", { key, attempt, tier: t });
    return { job: find((await loadJobs(l)).jobs, key), attempt, tier: t };
  });
}


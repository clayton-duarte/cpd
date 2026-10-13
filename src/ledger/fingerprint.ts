import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { z } from "zod";
import { failure, notFound, refused } from "../lib/errors.ts";
import { fingerprintsPath } from "../lib/paths.ts";
import { listIssues, readLedger, type ReadLedger } from "./bd.ts";
import type { BdIssue } from "./schema.ts";
import { writeJsonAtomic } from "./workflow.ts";

// Contract §8: Locked-Job fingerprints, stored outside beads.

/** JSON with object keys sorted recursively and no whitespace. */
export function canonicalJson(v: unknown): string {
  if (Array.isArray(v)) return `[${v.map(canonicalJson).join(",")}]`;
  if (v !== null && typeof v === "object") {
    const o = v as Record<string, unknown>;
    return `{${Object.keys(o).sort().map((k) => `${JSON.stringify(k)}:${canonicalJson(o[k])}`).join(",")}}`;
  }
  return JSON.stringify(v) ?? "null";
}

export const sha256 = (s: string): string => createHash("sha256").update(s).digest("hex");

export type FingerprintInput = {
  goal: unknown;
  tier: unknown;
  review_tier: unknown;
  context: unknown;
  touches: unknown;
  acceptance: unknown;
  out_of_scope: unknown;
  /** Keys of the Jobs this one needs. */
  needs: string[];
};

/** sha256 of the canonical JSON of the Spec fields and the sorted `needs`. */
export function fingerprint(i: FingerprintInput): string {
  return sha256(
    canonicalJson({
      goal: i.goal,
      tier: i.tier,
      review_tier: i.review_tier,
      context: i.context,
      touches: i.touches,
      acceptance: i.acceptance,
      out_of_scope: i.out_of_scope,
      needs: [...i.needs].sort(),
    }),
  );
}

/** Job key of each Job issue id. A Job whose metadata lost its key falls back to its id, which then fails verification. */
export function keyMap(issues: BdIssue[]): Map<string, string> {
  return new Map(issues.map((i) => [i.id, typeof i.metadata?.key === "string" ? i.metadata.key : i.id]));
}

/** Needs (Job keys) from the `blocks` dependencies of a Job issue. */
export function needsOf(issue: BdIssue, keys: Map<string, string>): string[] {
  return (issue.dependencies ?? [])
    .filter((d) => d.type === "blocks")
    .map((d) => keys.get(d.depends_on_id) ?? d.depends_on_id)
    .sort();
}

/** Fingerprint of a Job issue as it is in the Ledger now. Reads raw fields so that a tampered shape changes the hash instead of failing to parse. */
export function fingerprintOfIssue(issue: BdIssue, keys: Map<string, string>): string {
  const m = issue.metadata ?? {};
  return fingerprint({
    goal: issue.description ?? null,
    tier: m.tier ?? null,
    review_tier: m.review_tier ?? null,
    context: m.context ?? null,
    touches: m.touches ?? null,
    acceptance: m.acceptance ?? null,
    out_of_scope: m.out_of_scope ?? null,
    needs: needsOf(issue, keys),
  });
}

const Fingerprints = z.strictObject({ v: z.literal(1), jobs: z.record(z.string(), z.string().regex(/^[0-9a-f]{64}$/)) });

/** Job key to fingerprint. Empty before the first `plan lock`. */
export function readFingerprints(wf: string): Record<string, string> {
  const path = fingerprintsPath(wf);
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return {};
    throw failure("fingerprints_unreadable", `cannot read ${path}: ${(e as Error).message}`, { path });
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw failure("fingerprints_invalid", `${path} is not valid JSON`, { path });
  }
  const parsed = Fingerprints.safeParse(raw);
  if (!parsed.success) throw failure("fingerprints_invalid", `${path} does not match the schema`, { path, issues: parsed.error.issues });
  return parsed.data.jobs;
}

/** Record fingerprints (merged into the file, atomically). The caller holds the ledger lock. A key is never overwritten with a different value. */
export function writeFingerprints(wf: string, entries: Record<string, string>): void {
  const current = readFingerprints(wf);
  for (const [k, v] of Object.entries(entries)) {
    if (current[k] !== undefined && current[k] !== v) {
      throw refused("fingerprint_exists", `Job ${k} already has a different fingerprint`, { key: k });
    }
  }
  writeJsonAtomic(fingerprintsPath(wf), { v: 1, jobs: { ...current, ...entries } });
}

/** Recompute the Job's fingerprint and compare with the stored one; `locked_job_tampered` (exit 3) on mismatch. */
export async function verifyJob(wf: string, key: string, l: ReadLedger = readLedger(wf)): Promise<void> {
  const issues = await listIssues(l, "job");
  const keys = keyMap(issues);
  const issue = issues.find((i) => keys.get(i.id) === key);
  if (!issue) throw notFound("job_not_found", `no Job with key ${key}`, { key });
  const stored = readFingerprints(wf)[key];
  const actual = fingerprintOfIssue(issue, keys);
  if (stored === undefined) {
    if (issue.status === "draft") throw refused("job_not_locked", `Job ${key} is a draft and has no fingerprint`, { key });
    throw refused("locked_job_tampered", `Job ${key} is locked but has no recorded fingerprint`, { key, actual });
  }
  if (stored !== actual) {
    throw refused("locked_job_tampered", `Job ${key} changed after it was locked`, { key, expected: stored, actual });
  }
}

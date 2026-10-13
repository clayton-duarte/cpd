import { z } from "zod";
import { failure } from "../lib/errors.ts";
import { run, runJson } from "../lib/exec.ts";

// Contract §11: the only gateway to `gh`. Output of `gh` is parsed once here; the rest of `pr/` sees typed data.

const ghEnv = (): NodeJS.ProcessEnv => ({ ...process.env, GH_PROMPT_DISABLED: "1", NO_COLOR: "1" });
const TIMEOUT_MS = 60_000;

/** GitHub ids are numbers (REST) or node-id strings (GraphQL, what `gh pr view` prints); both are kept as strings. */
const Id = z.union([z.string().min(1), z.number().int()]).transform(String);
const Login = z.object({ login: z.string() }).nullable().optional();

const Check = z.object({
  name: z.string().optional(), // CheckRun
  context: z.string().optional(), // StatusContext
  conclusion: z.string().nullish(), // CheckRun
  state: z.string().nullish(), // StatusContext
});
const Review = z.object({ id: Id, author: Login, state: z.string(), body: z.string().nullish() });
const IssueComment = z.object({ id: Id, author: Login, body: z.string().nullish() });

export const PrView = z.object({
  state: z.string(),
  mergedAt: z.string().nullable(),
  statusCheckRollup: z.array(Check),
  reviews: z.array(Review),
  comments: z.array(IssueComment),
});
export type PrView = z.infer<typeof PrView>;

const ReviewComment = z.object({
  id: Id,
  user: Login,
  path: z.string().nullish(),
  line: z.number().int().nullish(),
  body: z.string().nullish(),
});
export type ReviewComment = z.infer<typeof ReviewComment>;

const RepoView = z.object({ nameWithOwner: z.string().regex(/^[^/\s]+\/[^/\s]+$/) });

/** `gh repo view --json nameWithOwner` in `cwd`. */
export async function repoSlug(cwd: string): Promise<string> {
  return (await runJson("gh", ["repo", "view", "--json", "nameWithOwner"], { cwd, env: ghEnv(), timeoutMs: TIMEOUT_MS }, RepoView)).nameWithOwner;
}

/** `gh pr create`; it has no `--json`, so the URL (the last line of stdout) and the number in it are parsed. */
export async function createPr(cwd: string, p: { base: string; head: string; title: string; body: string }): Promise<{ number: number; url: string }> {
  const args = ["pr", "create", "--base", p.base, "--head", p.head, "--title", p.title, "--body", p.body];
  const r = await run("gh", args, { cwd, env: ghEnv(), timeoutMs: TIMEOUT_MS });
  if (r.code !== 0) throw failure("gh_failed", `gh pr create exited ${r.code ?? r.signal}`, { args, stderr: r.stderr.slice(-2000) });
  const url = r.stdout.trim().split("\n").at(-1)?.trim() ?? "";
  const m = /^https?:\/\/\S+\/pull\/(\d+)$/.exec(url);
  if (!m) throw failure("bad_tool_output", "gh pr create did not print a pull request URL", { stdout: r.stdout.slice(-2000) });
  return { number: Number(m[1]), url };
}

export function viewPr(cwd: string, n: number): Promise<PrView> {
  const args = ["pr", "view", String(n), "--json", "state,mergedAt,statusCheckRollup,reviews,comments"];
  return runJson("gh", args, { cwd, env: ghEnv(), timeoutMs: TIMEOUT_MS }, PrView);
}

/** Inline review comments (`gh pr view` does not list them). */
export function reviewComments(cwd: string, slug: string, n: number): Promise<ReviewComment[]> {
  return runJson("gh", ["api", `repos/${slug}/pulls/${n}/comments`], { cwd, env: ghEnv(), timeoutMs: TIMEOUT_MS }, z.array(ReviewComment));
}

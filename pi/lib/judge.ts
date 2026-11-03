// The Bifrost judge helper (write-time curation). Reads ONE JSON request on stdin, prints ONE JSON line on stdout.
// Stub: its Job fills it. `pi/test/judge.test.ts` pins it.

export interface JudgeRuntime {
  getProviders(): readonly { id: string; baseUrl?: string }[];
  getAuth(providerId: string): Promise<{ auth: { apiKey?: string } } | undefined>;
}

export interface JudgeRecord {
  id: string;
  kind: string;
  at: string;
  topic?: string;
  text: string;
}

export interface JudgeRequest {
  model: string;
  new: JudgeRecord;
  neighbour: JudgeRecord | null;
}

export type Verdict = "duplicate" | "supersedes" | "contradicts" | "independent";
export type Failure = { code: "judge_unreachable" | "judge_refused" | "judge_bad_reply"; message: string; details: Record<string, unknown> };
export type Answer =
  | { ok: true; verdict: Verdict; quote: string; reason: string; topic: string; short: string }
  | { ok: false; error: Failure };

export interface JudgeDeps {
  rt: JudgeRuntime;
  fetch?: typeof fetch;
  request: JudgeRequest;
}

export async function judge(_deps: JudgeDeps): Promise<Answer> {
  throw new Error("not implemented");
}

/** Prints ONE JSON line (the [`Answer`]) on `stdout`; returns the exit status (0 for every outcome it can describe). */
export async function main(_deps: Omit<JudgeDeps, "request"> & { stdin: string; stdout: { write(text: string): unknown }; stderr: { write(text: string): unknown } }): Promise<number> {
  throw new Error("not implemented");
}

// STUB: the implementation Job replaces it. `transition` and `main` throw.
export type Answer = { isError: boolean; text: string };
export type CallTool = (name: string, args: Record<string, unknown>, timeoutMs: number) => Promise<Answer>;

export class ConnectionError extends Error {
  readonly needsAuth: boolean;
  constructor(message: string, needsAuth = false) {
    super(message);
    this.needsAuth = needsAuth;
  }
}

export class Refusal extends Error {
  readonly code: "usage" | "jira_auth" | "jira_refused" | "jira_failed" | "jira_helper_failed";
  readonly details: Record<string, unknown>;
  constructor(code: Refusal["code"], message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.code = code;
    this.details = details;
  }
  get exit(): number {
    return this.code === "usage" ? 2 : this.code === "jira_auth" ? 3 : 1;
  }
}

export type Request = {
  server: Record<string, unknown>;
  ticket: string;
  target: "in_progress" | "in_review" | "done";
  names: { in_progress: string; in_review: string; done: string };
};

export type Result =
  | { ticket: string; target: string; outcome: "moved"; from: string; to: string }
  | { ticket: string; target: string; outcome: "unchanged"; status: string; why: "at_target" | "done_category" | "past_target" };

export async function transition(_call: CallTool, _request: Request): Promise<Result> {
  throw new Error("not implemented");
}

export async function main(
  _argv: string[],
  _env: NodeJS.ProcessEnv,
  _cwd: string,
  _stdin: string,
  _open?: (codingAgentEntry: string, agentDir: string, cwd: string, server: Record<string, unknown>) => Promise<{ call: CallTool; close: () => Promise<void> }>,
): Promise<{ value: unknown; exit: number }> {
  throw new Error("not implemented");
}

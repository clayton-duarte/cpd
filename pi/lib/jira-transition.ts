// The Jira transition helper of the `jira` source of engine step E24 (docs/architecture/swr-jira.md §3).
//
//   node pi/lib/jira-transition.ts --coding-agent <…/pi-coding-agent/dist/index.js>   (request JSON on stdin)
//
// Moves one ticket through the Atlassian MCP on pi's own MCP client; prints one JSON line.
// One getJiraIssue read per run; for target `done` it writes 1 into each estimate field that reads null (one edit), then moves.
import { loadPi, parseArgs, resultText, Refusal as SlackRefusal } from "./slack-channel.ts";

export const CALL_TIMEOUT_MS = 30_000;
const AUTH_MARKERS = ["invalid_token", "unauthorized", "invalid_auth", "token_expired", "requires sign-in"];
const AUTH_MESSAGE = "Atlassian needs a sign-in. Run: pi mcp login atlassian";
const TARGETS = ["in_progress", "in_review", "done"] as const;
const looksLikeAuth = (text: string): boolean => AUTH_MARKERS.some((m) => text.toLowerCase().includes(m));
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
  /** The cached site id, or null to resolve it with getAccessibleAtlassianResources. */
  cloud_id: string | null;
  /** The cached accountId of the authenticated user, or null to resolve it with atlassianUserInfo. */
  account_id: string | null;
  /** The policy name of the target. */
  target_name: string;
  /** The names of the targets of higher rank, in rank order. */
  later_names: string[];
  /** Custom field ids (`customfield_N`, distinct); written as 1 when empty before a Done move. `[]` writes nothing. */
  estimate_fields: string[];
  /** The old request's names of all three targets: never read (swr-jira.md §3.1 drops it); the type keeps it so a caller that still builds one compiles. */
  names?: { in_progress: string; in_review: string; done: string };
};

type Facts = { cloud_id: string; account_id: string; summary: string; estimates: Record<string, unknown> };
export type Result =
  | ({ ticket: string; target: string; outcome: "moved"; from: string; to: string } & Facts)
  | ({ ticket: string; target: string; outcome: "unchanged"; status_name: string; status_category: string; why: "at_target" | "done_category" | "past_target" } & Facts);

const authRefusal = (details: Record<string, unknown> = {}) => new Refusal("jira_auth", AUTH_MESSAGE, details);

/** One tool call, its answer parsed as JSON; every failure class of the design is a Refusal. `notFound` makes a 404 the issue_not_found refusal. */
async function tool(call: CallTool, name: string, args: Record<string, unknown>, notFound = false, parse = true): Promise<any> {
  let answer: Answer;
  try {
    answer = await call(name, args, CALL_TIMEOUT_MS);
  } catch (e) {
    if (e instanceof ConnectionError && e.needsAuth) throw authRefusal({ tool: name });
    const message = (e as Error).message ?? String(e);
    if (looksLikeAuth(message)) throw authRefusal({ tool: name });
    throw new Refusal("jira_failed", `${name} failed: ${message}`, { tool: name });
  }
  if (answer.isError) {
    let statusCode: number | undefined;
    try {
      const code = JSON.parse(answer.text)?.statusCode;
      if (typeof code === "number") statusCode = code;
    } catch {}
    if (statusCode === 401 || statusCode === 403 || looksLikeAuth(answer.text)) throw authRefusal({ tool: name });
    if (notFound && statusCode === 404) throw new Refusal("jira_refused", `${name}: the issue was not found.`, { tool: name, reason: "issue_not_found" });
    throw new Refusal("jira_failed", `${name} returned an error: ${answer.text.slice(0, 300)}`, { tool: name, ...(statusCode === undefined ? {} : { status_code: statusCode }) });
  }
  if (!parse) return undefined;
  try {
    return JSON.parse(answer.text);
  } catch {
    throw new Refusal("jira_failed", `${name} did not answer JSON (${answer.text.length} bytes).`, { tool: name });
  }
}

const unexpected = (name: string) => new Refusal("jira_failed", `${name} answered a shape the helper does not know.`, { tool: name });
const ESTIMATE_FIELD = /^customfield_[0-9]+$/;
const str = (v: unknown): v is string => typeof v === "string" && v !== "";

async function resolveSite(call: CallTool, ticket: string): Promise<string> {
  const resources = await tool(call, "getAccessibleAtlassianResources", {});
  if (!Array.isArray(resources)) throw unexpected("getAccessibleAtlassianResources");
  const sites = resources.filter((r: any) => Array.isArray(r?.scopes) && r.scopes.includes("read:jira-work") && r.scopes.includes("write:jira-work"));
  const ids = [...new Set(sites.map((r: any) => r.id))];
  if (ids.length === 0) throw authRefusal({ reason: "no_jira_site" });
  if (ids.length > 1) throw new Refusal("jira_refused", `The Atlassian account has ${ids.length} Jira sites; cannot tell which one holds ${ticket}.`, { reason: "site_ambiguous", sites: ids });
  const cloudId = ids[0];
  if (!str(cloudId)) throw unexpected("getAccessibleAtlassianResources");
  return cloudId;
}

export async function transition(call: CallTool, request: Request): Promise<Result> {
  const { ticket, target } = request;
  const want = request.target_name;

  const cloudId = request.cloud_id ?? (await resolveSite(call, ticket));

  // The one read: status, summary and the estimate fields.
  const issue = await tool(call, "getJiraIssue", { cloudId, issueIdOrKey: ticket, fields: ["status", "summary", ...request.estimate_fields] }, true);
  const status = issue?.fields?.status?.name;
  if (!str(status)) throw unexpected("getJiraIssue");
  const category = issue.fields.status.statusCategory?.key;
  const summary = typeof issue.fields.summary === "string" ? issue.fields.summary : "";
  const have = issue.fields ?? {};
  const estimates: Record<string, unknown> = {};
  for (const f of request.estimate_fields) if (f in have) estimates[f] = have[f];
  const facts = { cloud_id: cloudId, account_id: "", summary, estimates };
  const category_ = typeof category === "string" ? category : "";
  const unchanged = (why: "at_target" | "done_category" | "past_target"): Result => ({ ticket, target, outcome: "unchanged", cloud_id: cloudId, account_id: "", status_name: status, status_category: category_, why, summary, estimates });
  if (status === want) return unchanged("at_target");
  if (category === "done") return unchanged("done_category");
  if (request.later_names.includes(status)) return unchanged("past_target");

  const listed = await tool(call, "getTransitionsForJiraIssue", { cloudId, issueIdOrKey: ticket });
  if (!Array.isArray(listed?.transitions)) throw unexpected("getTransitionsForJiraIssue");
  const chosen = listed.transitions.find((t: any) => t?.to?.name === want);
  if (!chosen) throw new Refusal("jira_refused", `${ticket} has no transition to "${want}" from "${status}".`, { reason: "no_transition" });
  if (!str(String(chosen.id ?? ""))) throw unexpected("getTransitionsForJiraIssue");

  if (target === "done") {
    const empty = request.estimate_fields.filter((f) => have[f] === null);
    if (empty.length > 0) {
      await tool(call, "editJiraIssue", { cloudId, issueIdOrKey: ticket, fields: Object.fromEntries(empty.map((f) => [f, 1])) }, false, false);
      for (const f of empty) estimates[f] = 1;
    }
  }

  const done = await tool(call, "transitionJiraIssue", { cloudId, issueIdOrKey: ticket, transition: { id: String(chosen.id) } });
  if (done?.success !== true || !str(done.statusName)) throw unexpected("transitionJiraIssue");
  return { ticket, target, outcome: "moved", from: status, to: done.statusName, ...facts };
}

/** The request on stdin, validated: anything else is `usage`. */
function parseRequest(stdin: string): Request {
  const bad = (why: string) => new Refusal("usage", `stdin is not the request: ${why}.`);
  let r: any;
  try {
    r = JSON.parse(stdin);
  } catch {
    throw bad("not JSON");
  }
  if (typeof r !== "object" || r === null) throw bad("not an object");
  if (typeof r.ticket !== "string" || !/^[A-Z][A-Z0-9]+-[0-9]+$/.test(r.ticket)) throw bad("ticket");
  if (!TARGETS.includes(r.target)) throw bad("target");
  if (r.cloud_id !== null && !(typeof r.cloud_id === "string" && r.cloud_id !== "")) throw bad("cloud_id");
  if (typeof r.target_name !== "string" || r.target_name === "") throw bad("target_name");
  if (!Array.isArray(r.later_names) || r.later_names.some((n: unknown) => typeof n !== "string" || n === "")) throw bad("later_names");
  if (typeof r.server !== "object" || r.server === null || typeof r.server.url !== "string" || !r.server.url) throw bad("server.url");
  const ef = r.estimate_fields;
  if (!Array.isArray(ef) || ef.some((f: unknown) => typeof f !== "string" || !ESTIMATE_FIELD.test(f)) || new Set(ef).size !== ef.length) throw bad("estimate_fields");
  return { server: r.server, ticket: r.ticket, target: r.target, cloud_id: r.cloud_id, account_id: null, target_name: r.target_name, later_names: r.later_names, estimate_fields: ef };
}

/** The `atlassian` connection of pi, built from the request's server (never from mcp.json). */
export async function connect(codingAgentEntry: string, _agentDir: string, cwd: string, server: Record<string, unknown>): Promise<{ call: CallTool; close: () => Promise<void> }> {
  const pi = await loadPi(codingAgentEntry);
  let connection: any;
  try {
    connection = new pi.McpServerConnection({ entry: { name: "atlassian", config: server }, cwd, createTransport: pi.createDefaultTransport, credentials: new pi.McpOAuthCredentialStore(), onTools: () => {} });
  } catch (e) {
    throw new Refusal("jira_helper_failed", `pi's MCP modules did not have the shape the helper uses: ${(e as Error).message}`, { message: (e as Error).message });
  }
  const call: CallTool = async (name, args, timeoutMs) => {
    let result;
    try {
      result = await connection.callTool(name, args, { timeoutMs, signal: AbortSignal.timeout(timeoutMs) });
    } catch (e) {
      const message = (e as Error).message ?? String(e);
      if (connection.state === "needs-auth" || looksLikeAuth(message)) throw new ConnectionError(message, true);
      throw e;
    }
    return { isError: result.isError === true, text: resultText(result) };
  };
  return { call, close: () => connection.close() };
}

export async function main(
  argv: string[],
  env: NodeJS.ProcessEnv,
  cwd: string,
  stdin: string,
  open: typeof connect = connect,
): Promise<{ value: unknown; exit: number }> {
  let close: (() => Promise<void>) | undefined;
  try {
    let args: Record<string, string>;
    try {
      args = parseArgs(argv, ["coding-agent"]);
    } catch (e) {
      throw new Refusal("usage", (e as Error).message);
    }
    const agentDir = env.PI_CODING_AGENT_DIR;
    if (!agentDir) throw new Refusal("usage", "PI_CODING_AGENT_DIR is not set: it names the agent directory of the pi release.");
    const request = parseRequest(stdin);
    const connection = await open(args["coding-agent"]!, agentDir, cwd, request.server);
    close = connection.close;
    const result = await transition(connection.call, request);
    return { value: { ok: true, ...result }, exit: 0 };
  } catch (e) {
    const refusal =
      e instanceof Refusal
        ? e
        : e instanceof SlackRefusal
          ? new Refusal("jira_helper_failed", e.message, { message: e.message })
          : new Refusal("jira_helper_failed", `The helper failed: ${(e as Error).message}`, { message: (e as Error).message });
    return { value: { ok: false, error: { code: refusal.code, message: refusal.message, details: refusal.details } }, exit: refusal.exit };
  } finally {
    await close?.().catch(() => {});
  }
}

async function readStdin(): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks).toString("utf8");
}

// Node's `import.meta.main` is true through the adapter symlink (docs/implementing.md, 'Scripts run through the adapter link')
if (import.meta.main) {
  const { value, exit } = await main(process.argv.slice(2), process.env, process.cwd(), await readStdin());
  process.stdout.write(JSON.stringify(value) + "\n", () => process.exit(exit));
}

// The Jira transition helper of engine step E19.
//
//   node pi/lib/jira-transition.ts --coding-agent <…/pi-coding-agent/dist/index.js>   (request JSON on stdin)
//
// Moves one ticket through the Atlassian MCP on pi's own MCP client; prints one JSON line.
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
  names: { in_progress: string; in_review: string; done: string };
};

export type Result =
  | { ticket: string; target: string; outcome: "moved"; from: string; to: string }
  | { ticket: string; target: string; outcome: "unchanged"; status: string; why: "at_target" | "done_category" | "past_target" };

const authRefusal = (details: Record<string, unknown> = {}) => new Refusal("jira_auth", AUTH_MESSAGE, details);

/** One tool call, its answer parsed as JSON; every failure class of the design is a Refusal. `notFound` makes a 404 the issue_not_found refusal. */
async function tool(call: CallTool, name: string, args: Record<string, unknown>, notFound = false): Promise<any> {
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
  try {
    return JSON.parse(answer.text);
  } catch {
    throw new Refusal("jira_failed", `${name} did not answer JSON (${answer.text.length} bytes).`, { tool: name });
  }
}

const unexpected = (name: string) => new Refusal("jira_failed", `${name} answered a shape the helper does not know.`, { tool: name });
const str = (v: unknown): v is string => typeof v === "string" && v !== "";

export async function transition(call: CallTool, request: Request): Promise<Result> {
  const { ticket, target, names } = request;
  const want = names[target];

  const resources = await tool(call, "getAccessibleAtlassianResources", {});
  if (!Array.isArray(resources)) throw unexpected("getAccessibleAtlassianResources");
  const sites = resources.filter((r: any) => Array.isArray(r?.scopes) && r.scopes.includes("read:jira-work") && r.scopes.includes("write:jira-work"));
  const ids = [...new Set(sites.map((r: any) => r.id))];
  if (ids.length === 0) throw authRefusal({ reason: "no_jira_site" });
  if (ids.length > 1) throw new Refusal("jira_refused", `The Atlassian account has ${ids.length} Jira sites; cannot tell which one holds ${ticket}.`, { reason: "site_ambiguous", sites: ids });
  const cloudId = ids[0];
  if (!str(cloudId)) throw unexpected("getAccessibleAtlassianResources");

  const issue = await tool(call, "getJiraIssue", { cloudId, issueIdOrKey: ticket, fields: ["status"] }, true);
  const status = issue?.fields?.status?.name;
  if (!str(status)) throw unexpected("getJiraIssue");
  const category = issue.fields.status.statusCategory?.key;
  const later = TARGETS.slice(TARGETS.indexOf(target) + 1).map((t) => names[t]).filter((n) => n !== "");
  const unchanged = (why: "at_target" | "done_category" | "past_target"): Result => ({ ticket, target, outcome: "unchanged", status, why });
  if (status === want) return unchanged("at_target");
  if (category === "done") return unchanged("done_category");
  if (later.includes(status)) return unchanged("past_target");

  const listed = await tool(call, "getTransitionsForJiraIssue", { cloudId, issueIdOrKey: ticket });
  if (!Array.isArray(listed?.transitions)) throw unexpected("getTransitionsForJiraIssue");
  const chosen = listed.transitions.find((t: any) => t?.to?.name === want);
  if (!chosen) throw new Refusal("jira_refused", `${ticket} has no transition to "${want}" from "${status}".`, { reason: "no_transition" });
  if (!str(String(chosen.id ?? ""))) throw unexpected("getTransitionsForJiraIssue");

  const done = await tool(call, "transitionJiraIssue", { cloudId, issueIdOrKey: ticket, transition: { id: String(chosen.id) } });
  if (done?.success !== true || !str(done.statusName)) throw unexpected("transitionJiraIssue");
  return { ticket, target, outcome: "moved", from: status, to: done.statusName };
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
  if (typeof r.names !== "object" || r.names === null || TARGETS.some((t) => typeof r.names[t] !== "string")) throw bad("names");
  if (r.names[r.target] === "") throw bad("names[target] is empty");
  if (typeof r.server !== "object" || r.server === null || typeof r.server.url !== "string" || !r.server.url) throw bad("server.url");
  return { server: r.server, ticket: r.ticket, target: r.target, names: { in_progress: r.names.in_progress, in_review: r.names.in_review, done: r.names.done } };
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

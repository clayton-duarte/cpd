// The Slack helper of the team review Inbox (docs/commands.md §21.6; docs/research/team-review.md).
//
//   node pi/lib/slack-channel.ts --coding-agent <…/pi-coding-agent/dist/index.js> --channel <C…> --me <U…>
//
// Reads the team channel through pi's own MCP client and token store, so cpd has no HTTP client and never
// reads, copies or logs the credential (hermes' `slack_fetch.py` did the same through Hermes' storage). Prints one JSON
// value on stdout, progress on stderr. Writes nothing, to no file and to no Slack channel.
import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";
import { claimedByMe, countMessages, mentionsMe, parseMessages, reactionUsers, unwrap, WINDOW_DAYS, type Post } from "./slack-parse.ts";

/** `refresh.MSG_LIMIT` (refresh.py 56). */
export const MSG_LIMIT = 200;
/** The whole channel read (slack_fetch.py 183), and each thread or reaction read (211, 324). */
export const CHANNEL_TIMEOUT_MS = 45_000;
export const SIDE_TIMEOUT_MS = 30_000;
/** At most this many thread or reaction reads at once (refresh.py 353). */
export const PARALLEL = 8;
/** The server entry of `<agentDir>/mcp.json` (slack_fetch.py 41). */
export const SERVER = "slack";
const AUTH_MARKERS = ["invalid_token", "unauthorized", "invalid_auth", "token_expired", "requires sign-in"];

/** A failure the helper answers with: its code, exit status and details are the contract's (commands.md §21.6). */
export type RefusalCode = "usage" | "slack_not_configured" | "slack_auth" | "slack_failed" | "slack_refused" | "slack_unparseable" | "slack_helper_failed";

export class Refusal extends Error {
  readonly code: RefusalCode;
  readonly details: Record<string, unknown>;
  constructor(code: RefusalCode, message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.code = code;
    this.details = details;
  }
  get exit(): number {
    return this.code === "usage" ? 2 : this.code === "slack_auth" || this.code === "slack_not_configured" ? 3 : 1;
  }
}

/** Calls one tool of the Slack server and returns the text blocks joined by a newline. Throws a [`Refusal`]. */
export type CallTool = (name: string, args: Record<string, unknown>, timeoutMs: number) => Promise<string>;

/** `slack_fetch._result_text`: the text content blocks, joined. */
export function resultText(result: { content?: Array<{ type?: string; text?: string }> }): string {
  return (result.content ?? []).filter((b) => b.type === "text" && b.text).map((b) => b.text).join("\n");
}

const looksLikeAuth = (text: string): boolean => AUTH_MARKERS.some((m) => text.toLowerCase().includes(m));

/** Runs `work` for each item, at most `limit` at a time; the results keep the items' order. */
async function pool<T, R>(items: T[], limit: number, work: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let next = 0;
  const lane = async () => {
    for (let i = next++; i < items.length; i = next++) out[i] = await work(items[i]!);
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, lane));
  return out;
}

export type Read = { read: number; posts: Post[] };

/** One `known` entry of the helper's stdin (docs/architecture/swr-slack.md §3.2): the stored state of one (message, pull request) row. */
export type Known = {
  ts: string;
  repo: string;
  number: number;
  author_is_me: boolean;
  reply_count: number | null;
  latest_reply: string | null;
  reactions: Record<string, number>;
  eyes_me: boolean | null;
  thread_tags_me: boolean | null;
};

/** The helper's stdin: one JSON value (§3.2). `terminal` lists `owner/name#n` of merged or closed pull requests. */
export type Input = { version: 1; known: Known[]; terminal: string[] };

/**
 * Reads the channel, validates that the payload parses into messages (`refresh.fetch_slack`: else `slack_unparseable`, so a
 * caller never replaces its last good copy with what looks like a quiet channel), parses it, then reads every distinct
 * post's thread and the `:eyes:` reactors of the claimed ones, each failing soft as in hermes.
 */
// TODO(swr-slack implementation): `_input` is not read yet, so no skip rule (§3.4) applies and every thread and reaction is read.
export async function readTeamChannel(call: CallTool, channel: string, me: string, now: Date, _input: Input = { version: 1, known: [], terminal: [] }): Promise<Read> {
  const text = await call("slack_read_channel", { channel_id: channel, limit: MSG_LIMIT }, CHANNEL_TIMEOUT_MS);
  if (!text.trim()) throw new Refusal("slack_failed", "slack_read_channel returned no text content.");
  const transcript = unwrap(text);
  const read = countMessages(transcript);
  if (read < 1) throw new Refusal("slack_unparseable", `Slack returned no parseable messages (${text.length} bytes).`, { bytes: text.length });
  const cutoff = now.getTime() / 1000 - WINDOW_DAYS * 86_400;
  const posts = parseMessages(transcript, cutoff, me);

  // A thread that cannot be read is false (`thread_text`: "" on any failure), but a sign-in that is gone is not a soft failure.
  const soft = async <T>(work: () => Promise<T>, fallback: T): Promise<T> => {
    try {
      return await work();
    } catch (e) {
      if (e instanceof Refusal && e.code === "slack_auth") throw e;
      return fallback;
    }
  };
  const threadTs = [...new Set(posts.map((p) => p.ts_raw))];
  const tagged = new Map<string, boolean>();
  await pool(threadTs, PARALLEL, async (ts) => {
    const replies = await soft(() => call("slack_read_thread", { channel_id: channel, message_ts: ts }, SIDE_TIMEOUT_MS), "");
    tagged.set(ts, mentionsMe(replies, me));
  });
  const eyesTs = [...new Set(posts.filter((p) => p.claim !== null && p.reactions.includes("eyes")).map((p) => p.ts_raw))];
  const mine = new Map<string, boolean | null>();
  await pool(eyesTs, PARALLEL, async (ts) => {
    const raw = await soft(() => call("slack_get_reactions", { channel_id: channel, message_ts: ts }, SIDE_TIMEOUT_MS), "");
    mine.set(ts, claimedByMe(reactionUsers(raw, "eyes"), me));
  });
  for (const post of posts) {
    post.thread_tags_me = tagged.get(post.ts_raw) ?? false;
    post.eyes_me = mine.get(post.ts_raw) ?? null;
  }
  return { read, posts };
}

/** The four names of pi's own modules the helper uses, and where (docs/research/team-review.md §5). Not pi's public API. */
export const PI_MODULES = {
  config: "extensions/mcp/config.js",
  runtime: "extensions/mcp/runtime.js",
} as const;
export const PI_NAMES = { config: ["loadMcpConfig"], runtime: ["McpServerConnection", "McpOAuthCredentialStore", "createDefaultTransport"] } as const;

/** pi's modules by file URL (its `package.json` exports none of them), each name checked. Any failure is `slack_helper_failed`. */
export async function loadPi(codingAgentEntry: string): Promise<Record<string, any>> {
  const dist = dirname(codingAgentEntry);
  const loaded: Record<string, any> = {};
  try {
    for (const key of ["config", "runtime"] as const) {
      const module = await import(pathToFileURL(join(dist, PI_MODULES[key])).href);
      for (const name of PI_NAMES[key]) {
        if (typeof module[name] !== "function") throw new Error(`${PI_MODULES[key]} does not export ${name}`);
        loaded[name] = module[name];
      }
    }
  } catch (e) {
    throw new Refusal("slack_helper_failed", `Cannot load pi's MCP modules: ${(e as Error).message}`, { message: (e as Error).message });
  }
  return loaded;
}

/** The `slack` connection of pi, built as `pi mcp list` builds it (`extensions/mcp/cli.js` `createConnection`). */
export async function connect(codingAgentEntry: string, agentDir: string, cwd: string): Promise<{ call: CallTool; close: () => Promise<void> }> {
  const pi = await loadPi(codingAgentEntry);
  let connection: any;
  try {
    // Global file only: a repository must not redirect the credential (`projectTrusted: false`).
    const config = pi.loadMcpConfig({ agentDir, cwd, projectTrusted: false });
    const entry = config.servers.find((s: { name: string }) => s.name === SERVER);
    if (!entry) {
      throw new Refusal("slack_not_configured", `pi has no MCP server "${SERVER}" in ${join(agentDir, "mcp.json")}. Run: pi mcp add slack --url https://mcp.slack.com/mcp --exposure hidden`);
    }
    connection = new pi.McpServerConnection({ entry, cwd, createTransport: pi.createDefaultTransport, credentials: new pi.McpOAuthCredentialStore(), onTools: () => {} });
  } catch (e) {
    if (e instanceof Refusal) throw e;
    throw new Refusal("slack_helper_failed", `pi's MCP modules did not have the shape the helper uses: ${(e as Error).message}`, { message: (e as Error).message });
  }
  const call: CallTool = async (name, args, timeoutMs) => {
    let result;
    try {
      result = await connection.callTool(name, args, { timeoutMs, signal: AbortSignal.timeout(timeoutMs) });
    } catch (e) {
      const message = (e as Error).message ?? String(e);
      if (connection.state === "needs-auth" || looksLikeAuth(message)) throw new Refusal("slack_auth", `Slack needs a sign-in. Run: pi mcp login ${SERVER}`, { message });
      throw new Refusal("slack_failed", `${name} failed: ${message}`, { tool: name });
    }
    const text = resultText(result);
    if (result.isError) {
      if (looksLikeAuth(text)) throw new Refusal("slack_auth", `Slack needs a sign-in. Run: pi mcp login ${SERVER}`, { message: text });
      throw new Refusal("slack_failed", `${name} returned an error: ${text.slice(0, 300)}`, { tool: name });
    }
    return text;
  };
  return { call, close: () => connection.close() };
}

/** `--name value` pairs; every one of `names` must be present, `optional` ones may be, and nothing else is accepted. */
export function parseArgs(argv: string[], names: readonly string[], optional: readonly string[] = []): Record<string, string> {
  const out: Record<string, string> = {};
  for (let i = 0; i < argv.length; i += 2) {
    const name = argv[i]?.replace(/^--/, "");
    const value = argv[i + 1];
    if (!argv[i]?.startsWith("--") || name === undefined || !(names.includes(name) || optional.includes(name)) || value === undefined || value.startsWith("--")) {
      throw new Refusal("usage", `Unexpected argument ${JSON.stringify(argv[i])}. Usage: slack-channel.ts ${[...names.map((n) => `--${n} <${n}>`), ...optional.map((n) => `[--${n} <${n}>]`)].join(" ")}`);
    }
    out[name] = value;
  }
  for (const n of names) if (!(n in out)) throw new Refusal("usage", `--${n} is missing. Usage: slack-channel.ts ${names.map((x) => `--${x} <${x}>`).join(" ")}`);
  return out;
}

/** The helper: the JSON value to print and the exit status. Never throws. */
// TODO(swr-slack implementation): `_stdin` (the text of the helper's stdin, §3.2) is not parsed yet.
export async function main(argv: string[], env: NodeJS.ProcessEnv, cwd: string, now = new Date(), _stdin = ""): Promise<{ value: unknown; exit: number }> {
  let close: (() => Promise<void>) | undefined;
  try {
    const args = parseArgs(argv, ["coding-agent", "channel", "me"]);
    if (!/^C[A-Z0-9]+$/.test(args.channel!)) throw new Refusal("usage", `--channel ${JSON.stringify(args.channel)} is not a Slack channel id.`);
    if (!/^U[A-Z0-9]+$/.test(args.me!)) throw new Refusal("usage", `--me ${JSON.stringify(args.me)} is not a Slack user id.`);
    const agentDir = env.PI_CODING_AGENT_DIR;
    if (!agentDir) throw new Refusal("usage", "PI_CODING_AGENT_DIR is not set: it names the agent directory of the pi release.");
    const connection = await connect(args["coding-agent"]!, agentDir, cwd);
    close = connection.close;
    const { read, posts } = await readTeamChannel(connection.call, args.channel!, args.me!, now);
    return { value: { ok: true, channel: args.channel, read, posts }, exit: 0 };
  } catch (e) {
    const refusal = e instanceof Refusal ? e : new Refusal("slack_helper_failed", `The helper failed: ${(e as Error).message}`, { message: (e as Error).message });
    return { value: { ok: false, error: { code: refusal.code, message: refusal.message, details: refusal.details } }, exit: refusal.exit };
  } finally {
    // So that a token pi refreshed is saved (runtime.js `close`).
    await close?.().catch(() => {});
  }
}

// Node's `import.meta.main` is true through the adapter symlink (docs/implementing.md, 'Scripts run through the adapter link')
if (import.meta.main) {
  const { value, exit } = await main(process.argv.slice(2), process.env, process.cwd());
  // pi's modules may leave timers behind: leave once the answer is flushed.
  process.stdout.write(JSON.stringify(value) + "\n", () => process.exit(exit));
}

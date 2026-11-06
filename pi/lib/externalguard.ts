/** Decision pro-d451: raw gh comment writes, a raw `cpd pr approve`, `slack-post.ts` and an ungranted Slack send are the User's to approve (tool cpd_external_approve). */
import { PREFIXES, simpleCommands } from "./shell.ts";

export type SlackTool = "slack_send_message" | "slack_send_message_draft";
export type SlackParts = { channel: string; message: string; thread_ts?: string };

const SLACK_PREFIX = "mcp__slack__";
const SLACK_TOOLS: readonly string[] = ["slack_send_message", "slack_send_message_draft"];
const ASSIGNMENT = /^[A-Za-z_][A-Za-z0-9_]*=/;
const TIMEOUT_VALUE_FLAGS = new Set(["-k", "-s", "--kill-after", "--signal"]);

/** The words of a simple command with the wrappers off: env assignments, PREFIXES (and `env` flags), and `timeout [flags] <duration>` (which PREFIXES lacks). */
function commandWords(line: string[]): string[] {
  let i = 0;
  while (i < line.length) {
    const w = line[i]!;
    if (PREFIXES.has(w) || ASSIGNMENT.test(w)) {
      i++;
      if (w === "env") while (i < line.length && line[i]!.startsWith("-")) i++;
    } else if (w === "timeout") {
      i++;
      while (i < line.length && line[i]!.startsWith("-")) i += TIMEOUT_VALUE_FLAGS.has(line[i]!) ? 2 : 1;
      if (i < line.length && /^\d/.test(line[i]!)) i++;
    } else break;
  }
  return line.slice(i);
}

const commands = (command: string): string[][] => simpleCommands(command).map(commandWords);

const GH_VALUE_FLAGS = new Set(["-X", "--method", "-f", "-F", "--field", "--raw-field", "-H", "--header", "--input", "-q", "--jq", "-t", "--template", "--hostname", "--cache", "-p", "--preview"]);
const GH_FIELD_FLAGS = new Set(["-f", "-F", "--field", "--raw-field", "--input"]);
const WRITE_ENDPOINT = /(^|\/)(comments|reviews|replies)(\/|$)/;
const WRITE_MUTATION = /\b(addComment|addPullRequestReview|addPullRequestReviewThreadReply|addPullRequestReviewComment)\b/;

function ghApi(args: string[]): string | undefined {
  let method: string | undefined;
  let fields = false;
  const positionals: string[] = [];
  const values: string[] = [];
  for (let i = 0; i < args.length; i++) {
    const w = args[i]!;
    if (w.startsWith("--method=")) method = w.slice(9);
    else if (w.startsWith("-") && w.length > 1) {
      const flag = w.split("=")[0]!;
      if (GH_VALUE_FLAGS.has(flag)) {
        const value = w.includes("=") ? w.slice(w.indexOf("=") + 1) : (args[++i] ?? "");
        if (flag === "-X" || flag === "--method") method = value;
        if (GH_FIELD_FLAGS.has(flag)) fields = true;
        values.push(value);
      }
    } else positionals.push(w);
  }
  const endpoint = positionals[0] ?? "";
  if (endpoint === "graphql") {
    const m = values.map((v) => WRITE_MUTATION.exec(v)?.[1]).find((x) => x !== undefined);
    return m === undefined ? undefined : `gh api graphql ${m}`;
  }
  const writes = method === undefined ? fields : method.toUpperCase() !== "GET" && method.toUpperCase() !== "HEAD";
  return writes && WRITE_ENDPOINT.test(endpoint.split("?")[0]!) ? `gh api ${endpoint}` : undefined;
}

/** The refused gh command as the reason shows it, or undefined when it may run. */
export function ghWriteBypass(command: string): string | undefined {
  for (const words of commands(command)) {
    if (words[0] !== "gh") continue;
    const rest = words.slice(1);
    while (rest.length > 0 && rest[0]!.startsWith("-")) rest.splice(0, rest[0] === "-R" || rest[0] === "--repo" ? 2 : 1);
    const [a, b] = rest;
    if ((a === "pr" || a === "issue") && (b === "comment" || (a === "pr" && b === "review"))) return `gh ${a} ${b}`;
    if (a === "api") {
      const m = ghApi(rest.slice(1));
      if (m !== undefined) return m;
    }
  }
  return undefined;
}

/** A raw `cpd pr approve` (the approval is minted only by the tool, after the User's Allow once). */
export function cpdApproveBypass(command: string): string | undefined {
  for (const words of commands(command)) if (words[0] === "cpd" && words[1] === "pr" && words[2] === "approve") return "cpd pr approve";
  return undefined;
}

const RUNNERS = new Set(["node", "tsx", "ts-node", "bun", "deno", "npx", "sh", "bash", "zsh"]);
const isSlackScript = (w: string): boolean => w.split("/").pop() === "slack-post.ts";

/** `slack-post.ts` run in bash: as the command word or as an argument of a runner. */
export function slackScriptBypass(command: string): string | undefined {
  for (const words of commands(command)) {
    const first = words[0];
    if (first === undefined) continue;
    if (isSlackScript(first) || (RUNNERS.has(first) && words.slice(1).some(isSlackScript))) return "slack-post.ts";
  }
  return undefined;
}

export function isSlackSend(toolName: string): boolean {
  return toolName.startsWith(SLACK_PREFIX) && SLACK_TOOLS.includes(toolName.slice(SLACK_PREFIX.length));
}

type Grant = { tool: string; channel: string; message: string; thread: string };
let grants: Grant[] = [];

/** An in-process one-shot approval of exactly one Slack send (A5: no file). */
export function grantSlackSend(tool: SlackTool, p: SlackParts): void {
  grants.push({ tool, channel: p.channel, message: p.message, thread: p.thread_ts ?? "" });
}

/** True and consumes the one grant that matches the tool and the exact channel, message and thread; else false. */
export function takeSlackSend(toolName: string, input: unknown): boolean {
  if (!isSlackSend(toolName) || typeof input !== "object" || input === null) return false;
  const i = input as Record<string, unknown>;
  const thread = typeof i.thread_ts === "string" ? i.thread_ts : "";
  const at = grants.findIndex((g) => g.tool === toolName.slice(SLACK_PREFIX.length) && g.channel === i.channel_id && g.message === i.message && g.thread === thread);
  if (at < 0) return false;
  grants.splice(at, 1);
  return true;
}

export function resetSlackGrants(): void {
  grants = [];
}

/** The rule's reason, one line. */
export function reasonText(matched: string): string {
  return `φ: external writes are the User's: ask with the tool cpd_external_approve (pr_reply, pr_post, slack_send); refused: ${matched.replace(/\s+/g, " ")}`;
}

/**
 * The single entry of the three hooks: the full one-line reason, or undefined. A Slack send passes only with a grant, which it consumes;
 * `grantable: false` (a Worker) refuses it always and consumes nothing. A top-level `codemode` call is judged by its nested calls.
 */
export function externalBlock(toolName: string, args: Record<string, unknown>, opts?: { grantable?: boolean }): string | undefined {
  if (toolName === "bash") {
    const command = args.command;
    if (typeof command !== "string") return undefined;
    const matched = ghWriteBypass(command) ?? cpdApproveBypass(command) ?? slackScriptBypass(command);
    return matched === undefined ? undefined : reasonText(matched);
  }
  if (isSlackSend(toolName)) {
    if (opts?.grantable !== false && takeSlackSend(toolName, args)) return undefined;
    return reasonText(toolName.slice(SLACK_PREFIX.length));
  }
  return undefined;
}

// The Slack send helper of `cpd pr post` (docs/commands.md §21.7).
//
//   node pi/lib/slack-post.ts --coding-agent <…/pi-coding-agent/dist/index.js> --channel <C…>   (message on stdin)
//
// Posts one message through pi's own MCP client and sign-in; prints one JSON line.
import { realpathSync } from "node:fs";
import { connect, parseArgs, Refusal, type CallTool } from "./slack-channel.ts";

export const MESSAGE_LIMIT = 5000;
export const SEND_TIMEOUT_MS = 30_000;
export type Posted = { channel: string; ts: string; link: string };

/** Sends `message`; the answer must be the message JSON for `channel`, else `slack_unparseable` (it may have been posted). */
export async function postMessage(call: CallTool, channel: string, message: string): Promise<Posted> {
  const text = await call("slack_send_message", { channel_id: channel, message, unfurl_app_links: true }, SEND_TIMEOUT_MS);
  const unparseable = () =>
    new Refusal("slack_unparseable", `Slack's answer to the send was not the message JSON (${text.length} bytes): the message may have been posted, so check the channel before running again.`, { bytes: text.length });
  let parsed: any;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw unparseable();
  }
  const link = parsed?.message_link;
  const ts = parsed?.message_context?.message_ts;
  if (typeof link !== "string" || !link || typeof ts !== "string" || !ts || parsed?.message_context?.channel_id !== channel) throw unparseable();
  return { channel, ts, link };
}

/** The helper: the JSON value to print and the exit status. Never throws. */
export async function main(argv: string[], env: NodeJS.ProcessEnv, cwd: string, stdin: string, open: typeof connect = connect): Promise<{ value: unknown; exit: number }> {
  let close: (() => Promise<void>) | undefined;
  try {
    const args = parseArgs(argv, ["coding-agent", "channel"]);
    if (!/^C[A-Z0-9]+$/.test(args.channel!)) throw new Refusal("usage", `--channel ${JSON.stringify(args.channel)} is not a Slack channel id.`);
    const agentDir = env.PI_CODING_AGENT_DIR;
    if (!agentDir) throw new Refusal("usage", "PI_CODING_AGENT_DIR is not set: it names the agent directory of the pi release.");
    const length = [...stdin].length;
    if (length < 1 || length > MESSAGE_LIMIT) throw new Refusal("usage", `The message on stdin has ${length} characters; it must have 1 to ${MESSAGE_LIMIT}.`, { characters: length });
    const connection = await open(args["coding-agent"]!, agentDir, cwd);
    close = connection.close;
    const posted = await postMessage(connection.call, args.channel!, stdin);
    return { value: { ok: true, ...posted }, exit: 0 };
  } catch (e) {
    const refusal = e instanceof Refusal ? e : new Refusal("slack_helper_failed", `The helper failed: ${(e as Error).message}`, { message: (e as Error).message });
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

if (process.argv[1] !== undefined && realpathSync(process.argv[1]) === realpathSync(import.meta.filename)) {
  const { value, exit } = await main(process.argv.slice(2), process.env, process.cwd(), await readStdin());
  process.stdout.write(JSON.stringify(value) + "\n", () => process.exit(exit));
}

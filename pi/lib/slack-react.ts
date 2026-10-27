import { connect, parseArgs, Refusal, type CallTool } from "./slack-channel.ts";

export const ADD_TIMEOUT_MS = 30_000;

export type Reacted = { channel: string; ts: string; emoji: string; already: boolean };

export async function addReaction(call: CallTool, channel: string, ts: string, emoji: string): Promise<Reacted> {
  try {
    await call("slack_add_reaction", { channel_id: channel, message_ts: ts, emoji }, ADD_TIMEOUT_MS);
    return { channel, ts, emoji, already: false };
  } catch (e) {
    if (e instanceof Refusal && e.code === "slack_failed" && e.message.includes("already_reacted")) return { channel, ts, emoji, already: true };
    throw e;
  }
}

export async function main(argv: string[], env: NodeJS.ProcessEnv, cwd: string, open: typeof connect = connect): Promise<{ value: unknown; exit: number }> {
  let close: (() => Promise<void>) | undefined;
  try {
    const args = parseArgs(argv, ["coding-agent", "channel", "ts", "emoji"]);
    if (!/^C[A-Z0-9]{8,}$/.test(args.channel!)) throw new Refusal("usage", `--channel ${JSON.stringify(args.channel)} is not a Slack channel id.`);
    if (!/^[0-9]{10}\.[0-9]{6}$/.test(args.ts!)) throw new Refusal("usage", `--ts ${JSON.stringify(args.ts)} is not a Slack message ts.`);
    if (!/^[a-z0-9_+-]{1,100}$/.test(args.emoji!)) throw new Refusal("usage", `--emoji ${JSON.stringify(args.emoji)} is not an emoji name.`);
    const agentDir = env.PI_CODING_AGENT_DIR;
    if (!agentDir) throw new Refusal("usage", "PI_CODING_AGENT_DIR is not set: it names the agent directory of the pi release.");
    const connection = await open(args["coding-agent"]!, agentDir, cwd);
    close = connection.close;
    const reacted = await addReaction(connection.call, args.channel!, args.ts!, args.emoji!);
    return { value: { ok: true, ...reacted }, exit: 0 };
  } catch (e) {
    const refusal = e instanceof Refusal ? e : new Refusal("slack_helper_failed", `The helper failed: ${(e as Error).message}`, { message: (e as Error).message });
    return { value: { ok: false, error: { code: refusal.code, message: refusal.message, details: refusal.details } }, exit: refusal.exit };
  } finally {
    await close?.().catch(() => {});
  }
}

if (import.meta.main) {
  const { value, exit } = await main(process.argv.slice(2), process.env, process.cwd());
  process.stdout.write(JSON.stringify(value) + "\n", () => process.exit(exit));
}

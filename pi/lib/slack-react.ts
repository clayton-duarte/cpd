import { connect, type CallTool } from "./slack-channel.ts";

export const ADD_TIMEOUT_MS = 30_000;

export type Reacted = { channel: string; ts: string; emoji: string; already: boolean };

export async function addReaction(call: CallTool, channel: string, ts: string, emoji: string): Promise<Reacted> {
  void call, channel, ts, emoji;
  throw new Error("not implemented");
}

export async function main(argv: string[], env: NodeJS.ProcessEnv, cwd: string, open: typeof connect = connect): Promise<{ value: unknown; exit: number }> {
  void argv, env, cwd, open;
  throw new Error("not implemented");
}

if (import.meta.main) {
  const { value, exit } = await main(process.argv.slice(2), process.env, process.cwd());
  process.stdout.write(`${JSON.stringify(value)}\n`);
  process.exitCode = exit;
}

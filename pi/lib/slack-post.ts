import { connect, type CallTool } from "./slack-channel.ts";

export const MESSAGE_LIMIT = 5000;
export const SEND_TIMEOUT_MS = 30_000;
export type Posted = { channel: string; ts: string; link: string };

export async function postMessage(_call: CallTool, _channel: string, _message: string): Promise<Posted> {
  throw new Error("not implemented");
}

export async function main(_argv: string[], _env: NodeJS.ProcessEnv, _cwd: string, _stdin: string, _open: typeof connect = connect): Promise<{ value: unknown; exit: number }> {
  throw new Error("not implemented");
}

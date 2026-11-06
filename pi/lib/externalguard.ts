/** STUB (red tests only): the external guards of decision pro-d451. The implementing Job replaces every body. */
export type SlackTool = "slack_send_message" | "slack_send_message_draft";
export type SlackParts = { channel: string; message: string; thread_ts?: string };

export function ghWriteBypass(_command: string): string | undefined {
  return undefined;
}
export function cpdApproveBypass(_command: string): string | undefined {
  return undefined;
}
export function slackScriptBypass(_command: string): string | undefined {
  return undefined;
}
export function isSlackSend(toolName: string): boolean {
  return toolName === "mcp__slack__slack_send_message" || toolName === "mcp__slack__slack_send_message_draft";
}
export function grantSlackSend(_tool: SlackTool, _p: SlackParts): void {}
export function takeSlackSend(_toolName: string, _input: unknown): boolean {
  return false;
}
export function resetSlackGrants(): void {}
export function reasonText(_matched: string): string {
  return "";
}
export function externalBlock(_toolName: string, _args: Record<string, unknown>): string | undefined {
  return undefined;
}

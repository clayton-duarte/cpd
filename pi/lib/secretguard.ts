/** commands.md §15.17: the raw-`op` guard. Pure: the adapter (`adapter.ts`) applies it to every `bash` call of every role. */
import { PREFIXES, simpleCommands } from "./shell.ts";

/** The breached protocol's name (contract §5.2), the `protocol` of a `protocol-breach` Event (§10.18). */
export const PROTOCOL_NAME = "secrets";

/** The refused command as the reason shows it, or undefined when it may run. */
export function secretBypass(command: string): string | undefined {
  for (const line of simpleCommands(command)) {
    let i = 0;
    while (i < line.length && (PREFIXES.has(line[i]!) || /^[A-Za-z_][A-Za-z0-9_]*=/.test(line[i]!))) i++;
    const words = line.slice(i);
    const head = words[0];
    if (head === undefined) continue;
    if (head === "op" || head.endsWith("/op")) return ["op", ...words.slice(1, 2)].join(" ");
    if (head === "cpd" && words[1] === "secret" && words[2] === "approve") return "cpd secret approve";
  }
  return undefined;
}

/** The rule's reason, one line. */
export function reasonText(matched: string): string {
  return matched === "cpd secret approve"
    ? `φ: approvals are the User's: ask with the tool cpd_secret_approve (commands.md §15.17); refused: ${matched}`
    : `φ: agents do not call 1Password's op; use cpd secret get|list|create|edit|delete (commands.md §24); refused: ${matched}`;
}

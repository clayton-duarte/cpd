import type { Host } from "./host.ts";
import type { Verdict } from "./refusal-report.ts";
import { blocked } from "./refusal-report.ts";

/**
 * commands.md §10.18, contract §5.2: a guard's refusal also records one `protocol-breach` Event in the Ledger,
 * with the protocol it enforces, the guard, the refused tool and the refused command line (none for a question).
 */
export function reportProtocolBreach(
  host: Host,
  protocol: string,
  source: string,
  tool: string,
  command: string | undefined,
  reason: string,
): void {
  const id = host.run?.id ?? host.env.CPD_SESSION;
  const args = [
    "session",
    "report",
    "--kind",
    "protocol-breach",
    "--protocol",
    protocol,
    "--source",
    source,
    "--tool",
    tool,
    "--message",
    reason,
    ...(command === undefined ? [] : ["--command", command]),
    ...(id === undefined ? [] : ["--session", id]),
  ];
  host.call(args).catch((e) => host.deliver("protocol breach", e)); // L4: a failed call is logged, never retried
}

/** Reports a protocol breach and returns the blocked verdict. */
export function breachBlocked(
  host: Host,
  protocol: string,
  source: string,
  toolName: string,
  reason: string,
  input: unknown,
  command?: string,
): Verdict {
  reportProtocolBreach(host, protocol, source, toolName, command, reason);
  return blocked(host, toolName, reason, input);
}

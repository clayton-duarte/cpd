/**
 * Reporting a refused tool call or command as a `guard` Event (contract §5.2, Plan `agent-refusals`).
 *
 * Every adapter guard refusal funnels through {@link blocked}, the verdict of the adapter's single `tool_call`
 * handler (commands.md §15): pi persists it as a `toolResult` whose first content `text` starts with `φ: ` and
 * whose `details` are empty, and the guard rule is that text's prefix. {@link registerRefusalReports} reports an
 * agent-caused CLI refusal — a `bash`/`codemode` result carrying the `{"ok":false,"error":{…}}` envelope
 * (commands.md §1.2) — the same way. Both go through `cpd session report` (commands.md §10.18), which appends the
 * `guard` kind with `data` `{code, role, tier, model, tool, excerpt}` instead of the generic adapter `error`.
 */

import type { Host } from "./host.ts";

/** What the guards of the `tool_call` handler produce, and what {@link blocked} returns. */
export type Verdict = { block: true; reason: string } | undefined;

/** The refused call: the tool name and its input, as pi's `tool_call` and `tool_result` carry them. */
type Call = { toolName: string; input?: unknown };

/** The guard rule of a `φ: <rule>: …` refusal text: the prefix before its first `: `. */
function rule(reason: string): string {
  const rest = reason.startsWith("φ: ") ? reason.slice(3) : reason;
  const i = rest.indexOf(":");
  return (i === -1 ? rest : rest.slice(0, i)).trim() || "guard";
}

/** The refused command line (`bash`, `codemode`) or path (the file tools) of a call, cut to 200 characters. */
function excerpt(call: Call): string {
  const input = (call.input ?? {}) as { command?: unknown; path?: unknown; file_path?: unknown };
  const raw =
    typeof input.command === "string" ? input.command
    : typeof input.path === "string" ? input.path
    : typeof input.file_path === "string" ? input.file_path
    : "";
  return raw.replace(/\s+/g, " ").trim().slice(0, 200);
}

/** Reports one refusal through `cpd session report`: its `code` is the guard rule or the envelope's code. */
function report(host: Host, call: Call, code: string, message: string, text: string): void {
  const id = host.run?.id ?? host.env.CPD_SESSION;
  if (id === undefined) return;
  const details = JSON.stringify({ guard: code, tool: call.toolName, excerpt: text });
  const args = ["session", "report", "--what", "guard refusal", "--code", "guard", "--message", (message || code).slice(0, 2000), "--details", details, "--session", id];
  // A failed report is one log line at the adapter's log (commands.md §15.6); it is never reported again.
  void host.call(args).catch(() => undefined);
}

/** The `{block: true, reason}` verdict of a guard refusal, and its report as a `guard` Event. */
export function blocked(host: Host, toolName: string, reason: string, input?: unknown): Verdict {
  const call = { toolName, input };
  report(host, call, rule(reason), reason, excerpt(call));
  return { block: true, reason };
}

/** The agent-caused CLI refusal envelope at the start of a line of an output, or undefined. */
function envelope(output: string): { code: string; message: string } | undefined {
  for (const line of output.split("\n")) {
    if (!line.startsWith('{"ok":false')) continue;
    try {
      const v = JSON.parse(line) as { ok?: unknown; error?: { code?: unknown; message?: unknown } };
      if (v.ok !== false || typeof v.error !== "object" || v.error === null) continue;
      if (typeof v.error.code !== "string") continue;
      return { code: v.error.code, message: typeof v.error.message === "string" ? v.error.message : "" };
    } catch {
      // a line that only looks like the envelope: not a refusal
    }
  }
  return undefined;
}

/** The first text block of a `tool_result`'s content, or `""`. */
function text(content: unknown): string {
  const first = Array.isArray(content) ? (content[0] as { type?: unknown; text?: unknown } | undefined) : undefined;
  return first !== undefined && first.type === "text" && typeof first.text === "string" ? first.text : "";
}

/**
 * Reports every agent-caused CLI refusal: a `bash` or `codemode` tool result whose output carries the
 * `{"ok":false,"error":{…}}` envelope (`usage`, `spec_invalid`, `refused`, `shape_violation`, …). A result that is
 * not one appends nothing.
 */
export function registerRefusalReports(host: Host): void {
  host.pi.on("tool_result", (event) => {
    if (!host.run || (event.toolName !== "bash" && event.toolName !== "codemode")) return;
    const call = { toolName: event.toolName, input: event.input };
    const refusal = envelope(text(event.content));
    if (refusal === undefined) return;
    report(host, call, refusal.code, refusal.message, excerpt(call));
  });
}

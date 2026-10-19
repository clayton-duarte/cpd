import { statSync } from "node:fs";
import { delimiter, join } from "node:path";
import type { Exec, ExecOptions } from "./pi.ts";

/** A failed `cpd` call or adapter step, with the error code the documents name. */
export class CpdError extends Error {
  readonly code: string;
  readonly details: Record<string, unknown>;
  constructor(code: string, message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.code = code;
    this.details = details;
  }
}

export const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

export function toCpdError(e: unknown): CpdError {
  if (e instanceof CpdError) return e;
  return new CpdError("adapter_failed", e instanceof Error ? e.message : String(e));
}

/** The `error` object of a `{"ok": false, "error": {…}}` envelope, as a CpdError. */
export function errorOfEnvelope(o: Record<string, unknown>): CpdError {
  const e = isObject(o.error) ? o.error : {};
  return new CpdError(
    typeof e.code === "string" ? e.code : "bad_output",
    typeof e.message === "string" ? e.message : "cpd reported a failure without a message",
    isObject(e.details) ? e.details : {},
  );
}

/** How long a `cpd` call may run, and what aborts it. */
export type CallOptions = { timeoutMs: number; signal?: AbortSignal };

/**
 * Run `cpd <args>` and return its parsed `{"ok": true, …}` stdout (commands.md §15.1, "Calls").
 * Stdout that is not one JSON object, a non-zero exit or `ok: false` is a CpdError carrying cpd's own code;
 * a call ended by the timeout is `tool_timeout`, one ended by the signal is `aborted`.
 */
export async function callCpd(exec: Exec, args: string[], options: CallOptions): Promise<Record<string, unknown>> {
  const execOptions: ExecOptions = { timeout: options.timeoutMs, signal: options.signal };
  let r;
  try {
    r = await exec("cpd", args, execOptions);
  } catch (e) {
    throw new CpdError("spawn_failed", `cannot run cpd ${args.join(" ")}: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (r.killed) {
    if (options.signal?.aborted) throw new CpdError("aborted", `cpd ${args.join(" ")} was aborted`, { tool: "cpd", argv: ["cpd", ...args] });
    throw new CpdError("tool_timeout", `cpd ${args.join(" ")} ran past its deadline of ${options.timeoutMs / 1000} s`, { tool: "cpd", argv: ["cpd", ...args], deadline_seconds: options.timeoutMs / 1000 });
  }
  let json: unknown;
  try {
    json = JSON.parse(r.stdout);
  } catch {
    throw new CpdError("bad_output", `cpd ${args.join(" ")} exited ${r.code} without JSON on stdout: ${(r.stderr || r.stdout).trim().slice(-500)}`);
  }
  if (!isObject(json)) throw new CpdError("bad_output", `cpd ${args.join(" ")} printed JSON that is not an object`);
  if (json.ok === false) throw errorOfEnvelope(json);
  if (json.ok !== true || r.code !== 0) throw new CpdError("bad_output", `cpd ${args.join(" ")} exited ${r.code} with an unexpected envelope`);
  return json;
}

/**
 * The identity of the installed `cpd`: the file the first `cpd` on `PATH` is, with its mtime and size (commands.md §15.2, step 0).
 * A new install changes it. `undefined` when no `cpd` can be found.
 */
export function installedIdentity(pathEnv: string | undefined = process.env.PATH): string | undefined {
  for (const dir of (pathEnv ?? "").split(delimiter)) {
    if (dir === "") continue;
    const file = join(dir, "cpd");
    try {
      const st = statSync(file);
      if (st.isFile()) return `${file}:${st.mtimeMs}:${st.size}`;
    } catch {
      // not here
    }
  }
  return undefined;
}

/** `cpd --version`'s `protocol`: a safe integer, or `null` when the output lacks it or it is not one (an old cpd). */
export async function cpdProtocol(exec: Exec, options: CallOptions): Promise<number | null> {
  const p = (await callCpd(exec, ["--version"], options)).protocol;
  return Number.isSafeInteger(p) ? (p as number) : null;
}

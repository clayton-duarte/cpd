export type ExecResult = { stdout: string; stderr: string; code: number };
export type Exec = (command: string, args: string[]) => Promise<ExecResult>;

/**
 * Run `cpd <args>` (on PATH) and return its parsed `{ok: true, ...}` stdout.
 * A non-zero exit, unparseable stdout or `ok: false` throws with cpd's own error code and message.
 */
export async function callCpd(exec: Exec, args: string[]): Promise<Record<string, unknown>> {
  const r = await exec("cpd", args);
  let json: unknown;
  try {
    json = JSON.parse(r.stdout);
  } catch {
    throw new Error(`cpd ${args.join(" ")} exited ${r.code} without JSON on stdout: ${(r.stderr || r.stdout).trim().slice(-500)}`);
  }
  const o = json as { ok?: unknown; error?: { code?: unknown; message?: unknown } };
  if (r.code !== 0 || o === null || typeof o !== "object" || o.ok !== true) {
    const code = typeof o?.error?.code === "string" ? o.error.code : `exit_${r.code}`;
    const message = typeof o?.error?.message === "string" ? o.error.message : r.stdout.trim().slice(-500);
    throw new Error(`cpd ${args.join(" ")} failed (${code}): ${message}`);
  }
  return json as Record<string, unknown>;
}

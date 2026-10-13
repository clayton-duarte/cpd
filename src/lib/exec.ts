import { spawn } from "node:child_process";
import type { z } from "zod";
import { failure, timeout } from "./errors.ts";

export type RunOptions = {
  cwd: string;
  env: NodeJS.ProcessEnv;
  timeoutMs?: number;
  input?: string;
};
/** `code` is null and `signal` set when the process was killed by a signal. */
export type RunResult = { code: number | null; signal: NodeJS.Signals | null; stdout: string; stderr: string };

export function run(cmd: string, args: string[], opts: RunOptions): Promise<RunResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd: opts.cwd,
      env: opts.env,
      stdio: [opts.input === undefined ? "ignore" : "pipe", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    child.stdout!.setEncoding("utf8").on("data", (d: string) => (stdout += d));
    child.stderr!.setEncoding("utf8").on("data", (d: string) => (stderr += d));
    const timer =
      opts.timeoutMs === undefined
        ? undefined
        : setTimeout(() => {
            timedOut = true;
            child.kill("SIGKILL");
          }, opts.timeoutMs);
    child.on("error", (e) => {
      clearTimeout(timer);
      reject(failure("spawn_failed", `cannot run ${cmd}: ${e.message}`, { cmd, args }));
    });
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      if (timedOut) {
        reject(timeout("exec_timeout", `${cmd} timed out after ${opts.timeoutMs} ms`, { cmd, args }));
        return;
      }
      resolve({ code, signal, stdout, stderr });
    });
    if (opts.input !== undefined) {
      child.stdin!.on("error", () => {});
      child.stdin!.end(opts.input);
    }
  });
}

const tail = (s: string, n = 2000) => (s.length > n ? s.slice(-n) : s);

/** Run, require exit 0, parse stdout as JSON with `schema`; fail loud with `bad_tool_output`. */
export async function runJson<S extends z.ZodType>(
  cmd: string,
  args: string[],
  opts: RunOptions,
  schema: S,
): Promise<z.infer<S>> {
  const r = await run(cmd, args, opts);
  const details = { cmd, args, code: r.code, signal: r.signal, stdout: tail(r.stdout), stderr: tail(r.stderr) };
  if (r.code !== 0) {
    const how = r.signal ? `was killed by ${r.signal}` : `exited ${r.code}`;
    throw failure("bad_tool_output", `${cmd} ${how}`, details);
  }
  let raw: unknown;
  try {
    raw = JSON.parse(r.stdout);
  } catch {
    throw failure("bad_tool_output", `${cmd} printed invalid JSON`, details);
  }
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    throw failure("bad_tool_output", `${cmd} output does not match the expected shape`, {
      ...details,
      issues: parsed.error.issues,
    });
  }
  return parsed.data;
}

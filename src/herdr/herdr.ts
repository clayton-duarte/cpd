import { z } from "zod";
import { loadConfig } from "../lib/config.ts";
import { failure } from "../lib/errors.ts";
import { run } from "../lib/exec.ts";

/** herdr prints `{id, result}` or `{id, error: {code, message}}`; errors may arrive on stdout or stderr (trial t1/t3). */
const Envelope = z.looseObject({
  result: z.unknown().optional(),
  error: z.looseObject({ code: z.string(), message: z.string() }).optional(),
});

export type HerdrOptions = {
  /** Process-level backstop. Default 30 s; pass `null` for none (a prompt that waits for a turn). */
  timeoutMs?: number | null;
};

const DEFAULT_TIMEOUT_MS = 30_000;
const tail = (s: string, n = 2000) => (s.length > n ? s.slice(-n) : s);

/**
 * Run `herdr [--session <config.herdrSession>] <args>` and return `result` parsed with `schema`.
 * A herdr error becomes `herdr_<code>` (exit 1); anything unparseable is `bad_tool_output`.
 * The session is read from `$CPD_HOME/config.json` on every call.
 */
export async function herdr<S extends z.ZodType>(
  args: string[],
  schema: S,
  { timeoutMs = DEFAULT_TIMEOUT_MS }: HerdrOptions = {},
): Promise<z.infer<S>> {
  const session = loadConfig().herdrSession;
  const argv = session === null ? args : ["--session", session, ...args];
  const r = await run("herdr", argv, {
    cwd: process.cwd(),
    env: process.env,
    ...(timeoutMs === null ? {} : { timeoutMs }),
  });
  const details = { args: argv, code: r.code, signal: r.signal, stdout: tail(r.stdout), stderr: tail(r.stderr) };
  const text = r.stdout.trim() !== "" ? r.stdout : r.stderr;
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw failure("bad_tool_output", `herdr ${r.signal ? `was killed by ${r.signal}` : `exited ${r.code}`} without a JSON envelope`, details);
  }
  const env = Envelope.safeParse(raw);
  if (!env.success) throw failure("bad_tool_output", "herdr output is not a result/error envelope", { ...details, issues: env.error.issues });
  if (env.data.error) {
    throw failure(`herdr_${env.data.error.code}`, env.data.error.message, { args: argv, herdrCode: env.data.error.code });
  }
  if (r.code !== 0) throw failure("bad_tool_output", `herdr exited ${r.code} without an error envelope`, details);
  const parsed = schema.safeParse(env.data.result);
  if (!parsed.success) {
    throw failure("bad_tool_output", "herdr result does not match the expected shape", { ...details, issues: parsed.error.issues });
  }
  return parsed.data;
}

/** herdr rejects control characters in argv; refuse them before calling (t1 F10). */
export function assertNoControlChars(what: string, value: string): void {
  if (/[\u0000-\u001f\u007f]/.test(value)) {
    throw failure("invalid_herdr_argument", `${what} contains a control character`, { what });
  }
}

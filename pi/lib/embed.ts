// The embedding helper: reads text on stdin, POSTs it to pi's `bifrost` provider (`<baseUrl>/embeddings`), prints one
// JSON line. The key comes from pi's ModelRuntime and is redacted from everything printed. `pi/test/embed.test.ts` pins it.
//
//   node pi/lib/embed.ts  (text on stdin; pi's Node; the engine's environment)

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const PROVIDER = "bifrost";
const MODEL = "vertex/gemini-embedding-2";
const UNREACHABLE = new Set(["ENOTFOUND", "ECONNREFUSED", "ETIMEDOUT", "EHOSTUNREACH", "ECONNRESET", "ENETUNREACH", "EAI_AGAIN", "UND_ERR_CONNECT_TIMEOUT"]);

export interface EmbedRuntime {
  getProviders(): readonly { id: string; baseUrl?: string }[];
  getAuth(providerId: string): Promise<{ auth: { apiKey?: string } } | undefined>;
}

export interface EmbedDeps {
  rt: EmbedRuntime;
  fetch: typeof fetch;
  /** The whole of stdin. */
  stdin: string;
  stdout: { write(text: string): unknown };
  stderr: { write(text: string): unknown };
  /** The vector length asked for and required back. */
  dims: number;
  /** The bound of the request, in ms; the signal comes from `timeoutSignal(timeoutMs)`. */
  timeoutMs: number;
  timeoutSignal: (ms: number) => AbortSignal;
}

class Failure extends Error {
  readonly code: string;
  readonly details: Record<string, unknown>;
  constructor(code: string, message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.code = code;
    this.details = details;
  }
}

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** True when the error, or a cause of it, is a connect/DNS/timeout failure. */
function isUnreachable(e: unknown): boolean {
  for (let cur: unknown = e, i = 0; cur !== undefined && cur !== null && i < 8; i++) {
    const c = cur as { name?: unknown; code?: unknown; cause?: unknown };
    if (c.name === "AbortError" || c.name === "TimeoutError") return true;
    if (typeof c.code === "string" && UNREACHABLE.has(c.code)) return true;
    cur = c.cause;
  }
  return false;
}

async function embed(deps: EmbedDeps, secret: { key?: string }): Promise<{ ok: true; vector: number[]; model: string; tokens?: number }> {
  const provider = deps.rt.getProviders().find((p) => p.id === PROVIDER);
  if (!provider?.baseUrl) throw new Failure("embedding_unreachable", "pi has no `bifrost` provider with a baseUrl.");
  const auth = await deps.rt.getAuth(PROVIDER);
  const key = auth?.auth.apiKey;
  if (!key) throw new Failure("embedding_refused", "pi has no key for the `bifrost` provider.", { status: null });
  secret.key = key;
  let res: Response;
  try {
    res = await deps.fetch(`${provider.baseUrl.replace(/\/+$/, "")}/embeddings`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${key}` },
      body: JSON.stringify({ model: MODEL, input: deps.stdin, dimensions: deps.dims }),
      signal: deps.timeoutSignal(deps.timeoutMs),
    });
  } catch (e) {
    if (isUnreachable(e)) throw new Failure("embedding_unreachable", `The embedding endpoint is unreachable: ${messageOf(e)}`);
    throw new Failure("embedding_unreachable", `The embedding request failed: ${messageOf(e)}`);
  }
  if (!res.ok) throw new Failure("embedding_refused", `The embedding endpoint answered HTTP ${res.status}.`, { status: res.status });
  let body: unknown;
  try {
    body = await res.json();
  } catch (e) {
    if (isUnreachable(e)) throw new Failure("embedding_unreachable", `The embedding reply was cut: ${messageOf(e)}`);
    throw new Failure("embedding_bad_reply", "The embedding reply is not JSON.");
  }
  const b = body as { data?: unknown; model?: unknown; usage?: { prompt_tokens?: unknown } } | null;
  const first = Array.isArray(b?.data) ? (b.data[0] as { embedding?: unknown } | undefined) : undefined;
  const vector = first?.embedding;
  if (!Array.isArray(vector)) throw new Failure("embedding_bad_reply", "The embedding reply has no vector.");
  if (vector.length !== deps.dims) throw new Failure("embedding_bad_reply", `The vector has ${vector.length} dimensions, expected ${deps.dims}.`);
  if (!vector.every((x) => typeof x === "number" && Number.isFinite(x))) throw new Failure("embedding_bad_reply", "The vector has a non-finite value.");
  const tokens = b?.usage?.prompt_tokens;
  return {
    ok: true,
    vector: vector as number[],
    model: typeof b?.model === "string" ? b.model : MODEL,
    ...(typeof tokens === "number" ? { tokens } : {}),
  };
}

/** Prints ONE JSON line; returns the exit status. */
export async function main(deps: EmbedDeps): Promise<number> {
  const secret: { key?: string } = {};
  const redact = (t: string) => (secret.key ? t.split(secret.key).join("[redacted]") : t);
  let answer: object;
  let status = 0;
  try {
    answer = await embed(deps, secret);
  } catch (e) {
    status = 1;
    const f = e instanceof Failure ? e : new Failure("embedding_unreachable", `The helper failed: ${messageOf(e)}`);
    answer = { ok: false, error: { code: f.code, message: f.message, details: f.details } };
  }
  deps.stdout.write(redact(JSON.stringify(answer)) + "\n");
  return status;
}

/** pi's `ModelRuntime`, for the release of `PI_CODING_AGENT_DIR` (as `pi/lib/models-list.ts`). */
async function piRuntime(env: NodeJS.ProcessEnv): Promise<EmbedRuntime> {
  const agentDir = env.PI_CODING_AGENT_DIR;
  if (!agentDir) throw new Error("PI_CODING_AGENT_DIR is not set: it names the agent directory of the pi release.");
  const piVersion = readFileSync(join(agentDir, "install", "current-version"), "utf8").split("\n")[0]!.trim();
  const entry = join(agentDir, "install", "releases", piVersion, "node_modules", "@earendil-works", "pi-coding-agent", "dist", "index.js");
  const pi = await import(pathToFileURL(entry).href);
  return pi.ModelRuntime.create({ allowModelNetwork: false });
}

async function readStdin(): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const c of process.stdin) chunks.push(c as Buffer);
  return Buffer.concat(chunks).toString("utf8");
}

if (import.meta.main) {
  let status: number;
  try {
    status = await main({
      rt: await piRuntime(process.env),
      fetch,
      stdin: await readStdin(),
      stdout: process.stdout,
      stderr: process.stderr,
      dims: 768,
      timeoutMs: 10_000,
      timeoutSignal: (ms) => AbortSignal.timeout(ms),
    });
  } catch (e) {
    process.stdout.write(JSON.stringify({ ok: false, error: { code: "embedding_unreachable", message: `The helper failed: ${messageOf(e)}`, details: {} } }) + "\n");
    status = 1;
  }
  process.stdout.write("", () => process.exit(status));
}

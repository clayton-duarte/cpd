// The classifier helper: reads `{state,question,labels}` on stdin, asks pi's classifier `llama.cpp/kev-4b`, prints one
// JSON line. `pi/test/classify.test.ts` pins it.
//
//   node pi/lib/classify.ts  (request on stdin; pi's Node; the engine's environment)

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const PROVIDER = "llama.cpp";
const MODEL_ID = "kev-4b";
const NETWORK = /ENOTFOUND|ECONNREFUSED|ETIMEDOUT|EHOSTUNREACH|ECONNRESET|ENETUNREACH|EAI_AGAIN|fetch failed|timed out|timeout|abort/i;

export interface ClassifyRuntime {
  getModelOfType(type: "classifier", providerId: string, modelId: string): unknown;
  classify(model: unknown, context: unknown, options?: { signal?: AbortSignal }): Promise<unknown>;
}

export interface ClassifyDeps {
  rt: ClassifyRuntime;
  /** The whole of stdin. */
  stdin: string;
  stdout: { write(text: string): unknown };
  stderr: { write(text: string): unknown };
  /** The bound of the request, in ms (3000); the signal comes from `timeoutSignal(timeoutMs)`. */
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

function isUnreachable(e: unknown): boolean {
  for (let cur: unknown = e, i = 0; cur !== undefined && cur !== null && i < 8; i++) {
    const c = cur as { name?: unknown; code?: unknown; message?: unknown; cause?: unknown };
    if (c.name === "AbortError" || c.name === "TimeoutError") return true;
    if (typeof c.code === "string" && NETWORK.test(c.code)) return true;
    cur = c.cause;
  }
  return false;
}

interface Label {
  name: string;
  definition: string;
}
interface Request {
  state: unknown;
  question: string;
  labels: Label[];
}

function parseRequest(stdin: string): Request {
  let v: unknown;
  try {
    v = JSON.parse(stdin);
  } catch {
    throw new Failure("classifier_bad_reply", "The request on stdin is not JSON.");
  }
  const r = v as { state?: unknown; question?: unknown; labels?: unknown } | null;
  const ok =
    typeof r === "object" && r !== null && !Array.isArray(r) && "state" in r && typeof r.question === "string" && Array.isArray(r.labels) && r.labels.length > 0 &&
    r.labels.every((l) => typeof l === "object" && l !== null && typeof (l as Label).name === "string" && typeof (l as Label).definition === "string");
  if (!ok) throw new Failure("classifier_bad_reply", "The request needs state, a string question and a non-empty list of {name, definition} labels.");
  return r as unknown as Request;
}

async function classify(deps: ClassifyDeps): Promise<object> {
  const req = parseRequest(deps.stdin);
  const criteria: Record<string, string> = {};
  for (const l of req.labels) criteria[l.name] = l.definition;
  const context = { state: req.state, questions: { q: { type: "choice", instructions: req.question, criteria } } };
  let result: { stopReason?: string; errorMessage?: string; answers?: Record<string, unknown>; usage?: { input?: number; output?: number } };
  try {
    const model = deps.rt.getModelOfType("classifier", PROVIDER, MODEL_ID);
    if (!model) throw new Failure("classifier_unreachable", `pi has no classifier model ${PROVIDER}/${MODEL_ID}.`);
    result = (await deps.rt.classify(model, context, { signal: deps.timeoutSignal(deps.timeoutMs) })) as typeof result;
  } catch (e) {
    if (e instanceof Failure) throw e;
    throw new Failure("classifier_unreachable", `The classifier is unreachable: ${messageOf(e)}`);
  }
  if (result.stopReason === "aborted") throw new Failure("classifier_unreachable", "The classifier request was aborted or timed out.");
  if (result.stopReason === "error") {
    const msg = result.errorMessage ?? "unknown error";
    if (NETWORK.test(msg)) throw new Failure("classifier_unreachable", `The classifier is unreachable: ${msg}`);
    const status = /\b([1-5]\d\d)\b/.exec(msg);
    throw new Failure("classifier_refused", `The classifier refused: ${msg}`, { status: status ? Number(status[1]) : null });
  }
  const a = result.answers?.q as { choice?: unknown; probabilities?: unknown; confidence?: unknown } | undefined;
  if (!a || typeof a.choice !== "string" || !(a.choice in criteria)) throw new Failure("classifier_bad_reply", "The classifier answered no label of the request.");
  return {
    ok: true,
    label: a.choice,
    ...(typeof a.confidence === "number" ? { confidence: a.confidence } : {}),
    ...(a.probabilities && typeof a.probabilities === "object" ? { probabilities: a.probabilities } : {}),
    model: `${PROVIDER}/${MODEL_ID}`,
    ...(result.usage ? { usage: { input_tokens: result.usage.input ?? 0, output_tokens: result.usage.output ?? 0 } } : {}),
  };
}

/** Prints ONE JSON line; returns the exit status. */
export async function main(deps: ClassifyDeps): Promise<number> {
  let answer: object;
  let status = 0;
  try {
    answer = await classify(deps);
  } catch (e) {
    status = 1;
    const f = e instanceof Failure ? e : new Failure("classifier_unreachable", `The helper failed: ${messageOf(e)}`);
    answer = { ok: false, error: { code: f.code, message: f.message, details: f.details } };
  }
  deps.stdout.write(JSON.stringify(answer) + "\n");
  return status;
}

/** pi's `ModelRuntime`, for the release of `PI_CODING_AGENT_DIR` (as `pi/lib/models-list.ts`). */
export interface PiRuntimeIo {
  readText(path: string): string;
  importModule(href: string): Promise<any>;
}
export type CurlSpawner = (args: string[], opts: { input?: Uint8Array; signal?: AbortSignal }) => Promise<Uint8Array>;

export function createFetchWithCurlFallback(_deps: { fetch: typeof fetch; curl: CurlSpawner; platform: string }): typeof fetch {
  throw new Error("not implemented");
}

export function installCurlFallback(): void {
  throw new Error("not implemented");
}

export async function piRuntime(env: NodeJS.ProcessEnv, io?: PiRuntimeIo): Promise<ClassifyRuntime> {
  if (io) throw new Error("not implemented");
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
    status = await main({ rt: await piRuntime(process.env), stdin: await readStdin(), stdout: process.stdout, stderr: process.stderr, timeoutMs: 3000, timeoutSignal: (ms) => AbortSignal.timeout(ms) });
  } catch (e) {
    process.stdout.write(JSON.stringify({ ok: false, error: { code: "classifier_unreachable", message: `The helper failed: ${messageOf(e)}`, details: {} } }) + "\n");
    status = 1;
  }
  process.stdout.write("", () => process.exit(status));
}

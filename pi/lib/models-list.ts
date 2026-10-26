// The model-discovery helper (docs/contract.md §3.7 E15; docs/commands.md §14.5; Context decisions pro-d13, pro-d17,
// pro-d20). Lists the models of every provider pi can use and a list can change for: `github-copilot` when pi has a
// login for it (pi's own API: the account's list) and every custom provider of `models.json` with a `baseUrl`
// (`GET <baseUrl>/models`, the key resolved by pi from that provider's own `apiKey`). Built-in providers are not listed.
// The key is never printed, logged, put in an argument or in an error. Prints ONE JSON value on stdout (`Answer`);
// writes nothing. `pi/test/models-list.test.ts` pins it.
//
//   node pi/lib/models-list.ts        (no argument; pi's Node; the engine's environment)
//
// The runtime is injected, so a test needs no login: `rt` is what pi's `ModelRuntime` offers. Built-in providers have
// a `refreshModels`; a `models.json` provider has none (docs/research/models-discovery.md D1, D2).

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/** What the helper uses of pi's `ModelRuntime` (the public methods of `model-runtime.d.ts`). */
export interface ListRuntime {
  getProviders(): readonly { id: string; baseUrl?: string; refreshModels?: unknown }[];
  /** The key pi resolved from the provider's own `apiKey` setting; throws or answers nothing when it is unset. */
  getAuth(providerId: string): Promise<{ auth: { apiKey?: string } } | undefined>;
  getAllModels(providerId: string): readonly { id: string }[];
  getAvailable(providerId: string): Promise<readonly { id: string }[]>;
  checkAuth(providerId: string): Promise<{ type: "api_key" | "oauth"; source?: string } | undefined>;
  refresh(options: { allowNetwork: boolean; force: boolean; providers: string[] }): Promise<{ aborted: boolean; errors: ReadonlyMap<string, Error> }>;
}

export interface ListDeps {
  rt: ListRuntime;
  piVersion: string;
  /** The `fetch` of a custom provider's list; Node's global one by default. */
  fetch?: typeof fetch;
  /** The timeout of one fetch; 60 000 by default. */
  fetchTimeoutMs?: number;
}

export type Failure = { code: string; message: string; details: Record<string, unknown> };
export type Entry =
  | { provider: string; auth: "oauth" | "api_key" | "custom"; base: string | null; ids: string[]; known?: string[] }
  | { provider: string; error: Failure };
export type Answer = { ok: true; pi_version: string; providers: Entry[] } | { ok: false; error: Failure };

const COPILOT = "github-copilot";
/** The timeout of one fetch (contract §3.7 E15). */
const FETCH_TIMEOUT_MS = 60_000;
/** The file-name-safe provider name (contract §1.4). */
const SAFE_NAME = /^[A-Za-z0-9._-]+$/;
/** The `cause.code` of a failed TLS handshake or certificate check (Node's, OpenSSL's). */
const TLS_CODE = /^(CERT_|ERR_TLS|ERR_SSL|UNABLE_TO_|SELF_SIGNED|DEPTH_ZERO|HOSTNAME_MISMATCH|ERR_OSSL)/;

const fail = (code: string, message: string, details: Record<string, unknown> = {}): { error: Failure } => ({ error: { code, message, details } });

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** `text` with the key removed, whatever a server or a library echoed back. */
const redact = (text: string, key: string): string => (key === "" ? text : text.split(key).join("***"));

/** One custom provider: `GET <baseUrl>/models` with the key pi resolved. */
async function listCustom(deps: ListDeps, provider: string, baseUrl: string): Promise<Entry> {
  const failed = (code: string, message: string, details: Record<string, unknown> = {}): Entry => ({ provider, ...fail(code, message, details) });
  if (!SAFE_NAME.test(provider)) return failed("models_unparseable", `The provider name ${JSON.stringify(provider)} is not a file-name-safe token.`);
  let url: URL;
  try {
    url = new URL(baseUrl);
  } catch {
    return failed("models_unparseable", `The baseUrl of ${provider} is not a URL.`);
  }
  let key = "";
  try {
    key = (await deps.rt.getAuth(provider))?.auth.apiKey ?? "";
  } catch (e) {
    return failed("models_key_unset", messageOf(e));
  }
  if (key === "") return failed("models_key_unset", `pi resolves no API key for provider ${provider}.`);

  const doFetch = deps.fetch ?? fetch;
  let body: string;
  try {
    const response = await doFetch(`${baseUrl.replace(/\/+$/, "")}/models`, {
      headers: { Authorization: `Bearer ${key}` },
      signal: AbortSignal.timeout(deps.fetchTimeoutMs ?? FETCH_TIMEOUT_MS),
    });
    if (response.status === 401) return failed("models_unauthorized", `${provider} refused the key (HTTP 401).`, { status: 401 });
    if (!response.ok) return failed("models_unreachable", `${provider} answered HTTP ${response.status}.`, { status: response.status });
    body = await response.text();
  } catch (e) {
    const name = (e as { name?: string })?.name;
    const code = String((e as { cause?: { code?: unknown } })?.cause?.code ?? (e as { code?: unknown })?.code ?? "");
    const message = redact(messageOf((e as { cause?: unknown })?.cause ?? e), key);
    if (name === "TimeoutError" || name === "AbortError") return failed("models_timeout", `${provider} did not answer in time.`);
    if (TLS_CODE.test(code)) return failed("models_tls", `TLS to ${provider} failed: ${message}`, { cause: code });
    return failed("models_unreachable", `${provider} is unreachable: ${message}`, code === "" ? {} : { cause: code });
  }

  let data: unknown;
  try {
    data = (JSON.parse(body) as { data?: unknown }).data;
  } catch {
    return failed("models_unparseable", `${provider}'s answer is not JSON.`);
  }
  if (!Array.isArray(data)) return failed("models_unparseable", `${provider}'s answer has no \`data\` array.`);
  if (data.length === 0) return failed("models_empty", `${provider} lists no model.`);
  const ids: string[] = [];
  for (const item of data) {
    const id = (item as { id?: unknown } | null)?.id;
    if (typeof id !== "string" || id === "") return failed("models_unparseable", `${provider}'s \`data\` holds an entry with no text \`id\`.`);
    ids.push(id);
  }
  return { provider, auth: "custom", base: url.origin, ids };
}

/** `github-copilot`, through pi's own API; `undefined` when pi has no login for it. */
async function listCopilot(deps: ListDeps): Promise<Entry | undefined> {
  const { rt } = deps;
  const failed = (code: string, message: string): Entry => ({ provider: COPILOT, ...fail(code, message) });
  try {
    const auth = await rt.checkAuth(COPILOT);
    if (!auth) return undefined;
    const result = await rt.refresh({ allowNetwork: true, force: true, providers: [COPILOT] });
    // pi swallows a refresh failure into `errors` and still answers from what it stored: raise it.
    const first = [...result.errors.values()][0];
    if (first) return failed("models_refresh_failed", messageOf(first));
    const ids = (await rt.getAvailable(COPILOT)).map((m) => m.id);
    if (ids.length === 0) return failed("models_empty", "pi lists no available github-copilot model.");
    return { provider: COPILOT, auth: auth.type, base: null, ids, known: rt.getAllModels(COPILOT).map((m) => m.id) };
  } catch (e) {
    return failed("models_refresh_failed", messageOf(e));
  }
}

/** Lists every provider pi can use. */
export async function listModels(deps: ListDeps): Promise<Answer> {
  const providers = deps.rt.getProviders();
  const work: Promise<Entry | undefined>[] = [];
  for (const p of providers) {
    if (p.id === COPILOT) work.push(listCopilot(deps));
    // A built-in provider has a `refreshModels`; one of `models.json` has none.
    else if (p.refreshModels === undefined && p.baseUrl) work.push(listCustom(deps, p.id, p.baseUrl));
  }
  const entries = (await Promise.all(work)).filter((e): e is Entry => e !== undefined);
  if (entries.length === 0) return { ok: false, ...fail("models_no_provider", "pi can use no provider whose model list this helper reads (github-copilot with a login, or a models.json provider with a baseUrl).") };
  return { ok: true, pi_version: deps.piVersion, providers: entries };
}

/** Prints ONE JSON value (the [`Answer`]) and a newline on `stdout`, progress on `stderr`; returns the exit status. */
export async function main(deps: ListDeps & { stdout: { write(text: string): unknown }; stderr: { write(text: string): unknown } }): Promise<number> {
  let answer: Answer;
  try {
    answer = await listModels(deps);
  } catch (e) {
    answer = { ok: false, ...fail("models_helper_failed", `The helper failed: ${messageOf(e)}`) };
  }
  deps.stdout.write(JSON.stringify(answer) + "\n");
  return answer.ok ? 0 : 1;
}

/** pi's `ModelRuntime`, for the release of `PI_CODING_AGENT_DIR` (as `pi/lib/slack-channel.ts` finds pi's modules). */
async function piRuntime(env: NodeJS.ProcessEnv): Promise<{ rt: ListRuntime; piVersion: string }> {
  const agentDir = env.PI_CODING_AGENT_DIR;
  if (!agentDir) throw new Error("PI_CODING_AGENT_DIR is not set: it names the agent directory of the pi release.");
  const piVersion = readFileSync(join(agentDir, "install", "current-version"), "utf8").split("\n")[0]!.trim();
  const entry = join(agentDir, "install", "releases", piVersion, "node_modules", "@earendil-works", "pi-coding-agent", "dist", "index.js");
  const pi = await import(pathToFileURL(entry).href);
  return { rt: await pi.ModelRuntime.create({ allowModelNetwork: false }), piVersion };
}

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  let status: number;
  try {
    status = await main({ ...(await piRuntime(process.env)), stdout: process.stdout, stderr: process.stderr });
  } catch (e) {
    process.stdout.write(JSON.stringify({ ok: false, ...fail("models_helper_failed", `The helper failed: ${messageOf(e)}`) }) + "\n");
    status = 1;
  }
  // pi's modules may leave timers behind: leave once the answer is flushed.
  process.stdout.write("", () => process.exit(status));
}

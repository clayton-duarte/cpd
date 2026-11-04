import assert from "node:assert/strict";
import { test } from "node:test";
import { main, type EmbedDeps } from "../lib/embed.ts";

const KEY = "sk-bf-secret-key";
const DIMS = 4;
const BASE = "http://bifrost.test/openai/v1";
const NEVER = new AbortController().signal;

type Call = { url: string; init: RequestInit };
type Reply = { status?: number; body?: unknown; throws?: unknown };

function run(stdin: string, reply: Reply, over: Partial<EmbedDeps> = {}) {
  const calls: Call[] = [];
  const out: string[] = [];
  const err: string[] = [];
  const fetchFake = (async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({ url: String(url), init: init ?? {} });
    if (reply.throws !== undefined) throw reply.throws;
    return new Response(JSON.stringify(reply.body ?? {}), { status: reply.status ?? 200, headers: { "content-type": "application/json" } });
  }) as typeof fetch;
  const deps: EmbedDeps = {
    rt: {
      getProviders: () => [{ id: "bifrost", baseUrl: BASE }],
      getAuth: async (id) => (id === "bifrost" ? { auth: { apiKey: KEY } } : undefined),
    },
    fetch: fetchFake,
    stdin,
    stdout: { write: (t) => out.push(t) },
    stderr: { write: (t) => err.push(t) },
    dims: DIMS,
    timeoutMs: 1000,
    timeoutSignal: () => NEVER,
    ...over,
  };
  return { deps, calls, out, err, go: () => main(deps) };
}

const good = (vector: number[] = [0.1, 0.2, 0.3, 0.4]) => ({ data: [{embedding: vector}], model: "vertex/gemini-embedding-2", usage: { prompt_tokens: 7, total_tokens: 7 } });
const answerOf = (out: string[]) => {
  const text = out.join("");
  assert.equal(text.endsWith("\n"), true);
  assert.equal(text.trimEnd().split("\n").length, 1, "exactly one JSON line");
  return JSON.parse(text) as { ok: boolean; vector?: number[]; model?: string; tokens?: number; error?: { code: string; message: string; details: Record<string, unknown> } };
};

test("embed prints one JSON line with vector, model and tokens", async () => {
  const r = run("hello", { body: good() });
  assert.equal(await r.go(), 0);
  assert.deepEqual(answerOf(r.out), { ok: true, vector: [0.1, 0.2, 0.3, 0.4], model: "vertex/gemini-embedding-2", tokens: 7 });
});

test("embed reads the text from stdin and sends it as one input", async () => {
  const r = run("some text\nover lines", { body: good() });
  await r.go();
  assert.equal(r.calls.length, 1);
  const call = r.calls[0]!;
  assert.equal(call.url, `${BASE}/embeddings`);
  assert.equal(call.init.method, "POST");
  assert.equal((call.init.headers as Record<string, string>).Authorization, `Bearer ${KEY}`);
  assert.deepEqual(JSON.parse(String(call.init.body)), { model: "vertex/gemini-embedding-2", input: "some text\nover lines", dimensions: DIMS });
});

test("embed never prints the key", async () => {
  const cases: Reply[] = [
    { body: good() },
    { status: 401, body: { error: { message: `bad credential Bearer ${KEY}` } } },
    { throws: Object.assign(new TypeError(`fetch failed for ${KEY}`), { cause: Object.assign(new Error(`connect ${KEY}`), { code: "ECONNREFUSED" }) }) },
    { body: { data: [{ embedding: [1] }], echo: KEY } },
  ];
  for (const reply of cases) {
    const r = run("t", reply);
    await r.go();
    assert.equal(r.out.join("").includes(KEY), false, "stdout");
    assert.equal(r.err.join("").includes(KEY), false, "stderr");
  }
});

test("embed refuses a wrong dimension or a non-finite float as embedding_bad_reply", async () => {
  // JSON cannot carry NaN/Infinity, so a fake `Response` with a custom `json` is not needed: 1e999 parses to Infinity.
  const bodies = [good([1, 2, 3]), good([1, 2, 3, 4, 5]), { data: [] }, {}, { data: [{ embedding: "x" }] }];
  for (const body of bodies) {
    const r = run("t", { body });
    assert.equal(await r.go(), 1);
    assert.equal(answerOf(r.out).error?.code, "embedding_bad_reply", JSON.stringify(body));
  }
  const raw = run("t", {});
  raw.deps.fetch = (async () => new Response('{"data":[{"embedding":[1,2,3,1e999]}],"usage":{"prompt_tokens":1}}', { status: 200 })) as typeof fetch;
  assert.equal(await raw.go(), 1);
  assert.equal(answerOf(raw.out).error?.code, "embedding_bad_reply", "Infinity");
  const nan = run("t", {});
  nan.deps.fetch = (async () => ({ ok: true, status: 200, json: async () => good([1, 2, 3, Number.NaN]), text: async () => "" }) as unknown as Response) as typeof fetch;
  assert.equal(await nan.go(), 1);
  assert.equal(answerOf(nan.out).error?.code, "embedding_bad_reply", "NaN");
});

test("embed maps HTTP statuses to embedding_refused", async () => {
  for (const status of [401, 403, 429, 500, 503]) {
    const r = run("t", { status, body: { error: { message: "no" } } });
    assert.equal(await r.go(), 1);
    const answer = answerOf(r.out);
    assert.equal(answer.ok, false);
    assert.equal(answer.error?.code, "embedding_refused", String(status));
    assert.equal(answer.error?.details.status, status);
  }
});

test("embed maps connect errors and timeouts to embedding_unreachable", async () => {
  for (const code of ["ENOTFOUND", "ECONNREFUSED", "ETIMEDOUT", "EHOSTUNREACH"]) {
    const r = run("t", { throws: Object.assign(new TypeError("fetch failed"), { cause: Object.assign(new Error(code), { code }) }) });
    assert.equal(await r.go(), 1);
    assert.equal(answerOf(r.out).error?.code, "embedding_unreachable", code);
  }
  // The timeout: the injected signal is already aborted, and the fake fetch honours it as Node's does.
  const aborted = AbortSignal.abort();
  const asked: number[] = [];
  const r = run("t", {}, { timeoutMs: 250, timeoutSignal: (ms) => (asked.push(ms), aborted) });
  r.deps.fetch = (async (_url: string | URL | Request, init?: RequestInit) => {
    if (init?.signal?.aborted) throw new DOMException("This operation was aborted", "AbortError");
    return new Response(JSON.stringify(good()));
  }) as typeof fetch;
  assert.equal(await r.go(), 1);
  assert.equal(answerOf(r.out).error?.code, "embedding_unreachable", "abort");
  assert.deepEqual(asked, [250]);
});

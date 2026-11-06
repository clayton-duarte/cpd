import assert from "node:assert/strict";
import { test } from "node:test";
import { createFetchWithCurlFallback, main, piRuntime, type ClassifyDeps, type CurlSpawner, type PiRuntimeIo } from "../lib/classify.ts";

const NEVER = new AbortController().signal;
const MODEL = { id: "kev-4b", provider: "llama.cpp", type: "classifier" };
const LABELS = [
  { name: "yes", definition: "the turn asks the user a question" },
  { name: "no", definition: "it does not" },
];
const REQUEST = { state: { last: "Shall I continue?" }, question: "Does the turn ask?", labels: LABELS };

type Result = { stopReason?: string; errorMessage?: string; answers?: Record<string, unknown>; usage?: unknown; model?: string };
const ok = (choice = "yes"): Result => ({
  stopReason: "stop",
  model: "kev-4b",
  answers: { q: { type: "choice", choice, probabilities: { yes: 0.9, no: 0.1 }, confidence: 0.9 } },
  usage: { input: 12, output: 3, cacheRead: 0, cacheWrite: 0, totalTokens: 15, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
});

function run(stdin: string, behave: (model: unknown, ctx: unknown, opts?: { signal?: AbortSignal }) => Promise<Result>, over: Partial<ClassifyDeps> = {}) {
  const asked: { type: string; provider: string; id: string }[] = [];
  const contexts: unknown[] = [];
  const out: string[] = [];
  const err: string[] = [];
  const deps: ClassifyDeps = {
    rt: {
      getModelOfType: (type, provider, id) => (asked.push({ type, provider, id }), MODEL),
      classify: (model, ctx, opts) => (contexts.push(ctx), behave(model, ctx, opts)) as Promise<never>,
    },
    stdin,
    stdout: { write: (t) => out.push(t) },
    stderr: { write: (t) => err.push(t) },
    timeoutMs: 3000,
    timeoutSignal: () => NEVER,
    ...over,
  };
  return { asked, contexts, out, err, go: () => main(deps) };
}

const answerOf = (out: string[]) => {
  const text = out.join("");
  assert.equal(text.trimEnd().split("\n").length, 1, "exactly one JSON line");
  return JSON.parse(text) as { ok: boolean; [k: string]: unknown; error?: { code: string; details: Record<string, unknown> } };
};

test("classify prints label, confidence, probabilities, model and usage", async () => {
  const r = run(JSON.stringify(REQUEST), async () => ok("yes"));
  assert.equal(await r.go(), 0);
  assert.deepEqual(r.asked, [{ type: "classifier", provider: "llama.cpp", id: "kev-4b" }]);
  assert.deepEqual(r.contexts[0], {
    state: REQUEST.state,
    questions: { q: { type: "choice", instructions: "Does the turn ask?", criteria: { yes: LABELS[0]!.definition, no: LABELS[1]!.definition } } },
  });
  const a = answerOf(r.out);
  assert.equal(a.ok, true);
  assert.equal(a.label, "yes");
  assert.equal(a.confidence, 0.9);
  assert.deepEqual(a.probabilities, { yes: 0.9, no: 0.1 });
  assert.equal(a.model, "llama.cpp/kev-4b");
  assert.deepEqual(a.usage, { input_tokens: 12, output_tokens: 3 });
});

test("classify maps ENOTFOUND, ECONNREFUSED, ETIMEDOUT, EHOSTUNREACH and timeouts to classifier_unreachable", async () => {
  for (const code of ["ENOTFOUND", "ECONNREFUSED", "ETIMEDOUT", "EHOSTUNREACH"]) {
    // As a thrown error with the code in its cause...
    const thrown = run(JSON.stringify(REQUEST), async () => {
      throw Object.assign(new TypeError("fetch failed"), { cause: Object.assign(new Error(code), { code }) });
    });
    assert.equal(await thrown.go(), 1);
    assert.equal(answerOf(thrown.out).error?.code, "classifier_unreachable", `thrown ${code}`);
    // ...and as pi's error result, which carries the failure as text.
    const result = run(JSON.stringify(REQUEST), async () => ({ stopReason: "error", errorMessage: `connect ${code} ollama.lab:80`, answers: {} }));
    assert.equal(await result.go(), 1);
    assert.equal(answerOf(result.out).error?.code, "classifier_unreachable", `result ${code}`);
  }
  const aborted = run(JSON.stringify(REQUEST), async () => ({ stopReason: "aborted", answers: {} }), { timeoutSignal: () => AbortSignal.abort() });
  assert.equal(await aborted.go(), 1);
  assert.equal(answerOf(aborted.out).error?.code, "classifier_unreachable", "aborted");
});

test("classify bounds the request by the timeout", async () => {
  const asked: number[] = [];
  const signal = AbortSignal.abort();
  let seen: AbortSignal | undefined;
  const r = run(
    JSON.stringify(REQUEST),
    async (_m, _c, opts) => {
      seen = opts?.signal;
      if (opts?.signal?.aborted) throw new DOMException("aborted", "AbortError");
      return ok();
    },
    { timeoutSignal: (ms) => (asked.push(ms), signal) },
  );
  assert.equal(await r.go(), 1);
  assert.deepEqual(asked, [3000]);
  assert.equal(seen, signal, "the signal reaches classify");
  assert.equal(answerOf(r.out).error?.code, "classifier_unreachable");
});

test("classify refuses a label outside the labels as classifier_bad_reply", async () => {
  const r = run(JSON.stringify(REQUEST), async () => ok("maybe"));
  assert.equal(await r.go(), 1);
  assert.equal(answerOf(r.out).error?.code, "classifier_bad_reply");
  const none = run(JSON.stringify(REQUEST), async () => ({ stopReason: "stop", answers: {} }));
  assert.equal(await none.go(), 1);
  assert.equal(answerOf(none.out).error?.code, "classifier_bad_reply", "no answer");
  const refused = run(JSON.stringify(REQUEST), async () => ({ stopReason: "error", errorMessage: "HTTP 500 from server", answers: {} }));
  assert.equal(await refused.go(), 1);
  assert.equal(answerOf(refused.out).error?.code, "classifier_refused", "an error result that is not a network failure");
});

test("classify refuses a malformed stdin request", async () => {
  const bad = ["", "not json", "[]", JSON.stringify({ state: {}, question: "q" }), JSON.stringify({ ...REQUEST, labels: [] }), JSON.stringify({ ...REQUEST, question: 3 }), JSON.stringify({ ...REQUEST, labels: [{ name: "a" }] })];
  for (const stdin of bad) {
    const r = run(stdin, async () => ok());
    assert.equal(await r.go(), 1, stdin);
    assert.equal(answerOf(r.out).error?.code, "classifier_bad_reply", stdin);
    assert.equal(r.contexts.length, 0, "classify is not called");
  }
});

// ---- piRuntime ----

const PROVIDER_OBJ = { id: "llama.cpp" };
function fakeIo(agentDir = "/agent") {
  const calls: string[] = [];
  const hrefs: string[] = [];
  let registered = false;
  let refreshed = false;
  const rt = {
    getModelOfType: () => (registered && refreshed ? MODEL : undefined),
    classify: async () => ({}),
    registerNativeProvider: (p: unknown) => {
      assert.equal(p, PROVIDER_OBJ);
      registered = true;
      calls.push("register");
    },
    refresh: async (o: unknown) => {
      assert.deepEqual(o, { providers: ["llama.cpp"], allowNetwork: false });
      refreshed = true;
      calls.push("refresh");
    },
  };
  const io: PiRuntimeIo = {
    readText: (path) => {
      assert.equal(path, `${agentDir}/install/current-version`);
      return "1.3.0\n";
    },
    importModule: async (href) => {
      hrefs.push(href);
      if (hrefs.length === 1) {
        return {
          ModelRuntime: {
            create: async (opts: unknown) => {
              assert.deepEqual(opts, { allowModelNetwork: false });
              calls.push("create");
              return rt;
            },
          },
        };
      }
      return { createLlamaProvider: () => ({ provider: PROVIDER_OBJ }) };
    },
  };
  return { io, calls, hrefs };
}

test("piRuntime registers the llama.cpp provider and refreshes it offline before returning", async () => {
  const { io, calls } = fakeIo();
  const rt = await piRuntime({ PI_CODING_AGENT_DIR: "/agent" }, io);
  assert.ok(rt.getModelOfType("classifier", "llama.cpp", "kev-4b"));
  assert.deepEqual(calls, ["create", "register", "refresh"]);
});

test("piRuntime imports the llama provider from dist/extensions/llama of the pi release", async () => {
  const { io, hrefs } = fakeIo();
  await piRuntime({ PI_CODING_AGENT_DIR: "/agent" }, io);
  assert.equal(hrefs[1], "file:///agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent/dist/extensions/llama/provider.js");
});

test("piRuntime fails loud when PI_CODING_AGENT_DIR is unset", async () => {
  const { io } = fakeIo();
  await assert.rejects(piRuntime({}, io), /PI_CODING_AGENT_DIR/);
});

// ---- fetch fallback ----

const enc = (t: string) => new TextEncoder().encode(t);
const unreachable = (code: string, depth = 1) => {
  let cause: object = { code };
  for (let i = 1; i < depth; i++) cause = { cause };
  return Object.assign(new TypeError("fetch failed"), { cause });
};
function harness(opts: { err?: unknown; stdout?: string; curlFails?: boolean; platform?: string; response?: Response }) {
  const curlCalls: { args: string[]; opts: { input?: Uint8Array; signal?: AbortSignal } }[] = [];
  const fakeFetch = (async () => {
    if (opts.response) return opts.response;
    throw opts.err;
  }) as unknown as typeof fetch;
  const curl: CurlSpawner = async (args, o) => {
    curlCalls.push({ args, opts: o });
    if (opts.curlFails) throw new Error("curl exit 7");
    return enc(opts.stdout ?? "");
  };
  const f = createFetchWithCurlFallback({ fetch: fakeFetch, curl, platform: opts.platform ?? "darwin" });
  return { f, curlCalls };
}
const OK200 = 'HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{"ok":true}';

test("fetch falls back to curl on EHOSTUNREACH on darwin and returns the parsed response", async () => {
  const { f, curlCalls } = harness({ err: unreachable("EHOSTUNREACH"), stdout: OK200 });
  const res = await f("http://ollama.lab/v1/x");
  assert.deepEqual(await res.json(), { ok: true });
  assert.equal(res.status, 200);
  assert.equal(res.headers.get("content-type"), "application/json");
  assert.equal(curlCalls.length, 1);
});

test("fetch fallback passes method, headers, body and the abort signal to curl", async () => {
  const { f, curlCalls } = harness({ err: unreachable("EHOSTUNREACH"), stdout: OK200 });
  const controller = new AbortController();
  await f("http://ollama.lab/v1/x", { method: "POST", headers: { "content-type": "application/json" }, body: '{"a":1}', signal: controller.signal });
  assert.equal(curlCalls.length, 1);
  const c = curlCalls[0]!;
  assert.deepEqual(c.args, ["-sS", "-i", "-X", "POST", "-H", "content-type: application/json", "--data-binary", "@-", "http://ollama.lab/v1/x"]);
  assert.equal(new TextDecoder().decode(c.opts.input), '{"a":1}');
  assert.equal(c.opts.signal?.aborted, false);
  controller.abort();
  assert.equal(c.opts.signal?.aborted, true);
});

test("fetch fallback also falls back on ENETUNREACH nested in the cause chain", async () => {
  const { f, curlCalls } = harness({ err: unreachable("ENETUNREACH", 2), stdout: OK200 });
  const res = await f("http://ollama.lab/v1/x");
  assert.equal(res.status, 200);
  assert.equal(curlCalls.length, 1);
});

test("fetch fallback skips curl and rethrows the original error off darwin", async () => {
  const err = unreachable("EHOSTUNREACH");
  const { f, curlCalls } = harness({ err, platform: "linux" });
  await assert.rejects(f("http://ollama.lab/v1/x"), (e) => e === err);
  assert.equal(curlCalls.length, 0);
});

test("fetch fallback rethrows other errors without curl", async () => {
  const err = unreachable("ECONNREFUSED");
  const { f, curlCalls } = harness({ err });
  await assert.rejects(f("http://ollama.lab/v1/x"), (e) => e === err);
  assert.equal(curlCalls.length, 0);
});

test("fetch fallback rethrows the original fetch error when curl fails", async () => {
  const err = unreachable("EHOSTUNREACH");
  const { f } = harness({ err, curlFails: true });
  await assert.rejects(f("http://ollama.lab/v1/x"), (e) => e === err);
});

test("fetch fallback returns the real response untouched when fetch succeeds", async () => {
  const response = new Response("hi");
  const { f, curlCalls } = harness({ response });
  assert.equal(await f("http://ollama.lab/v1/x"), response);
  assert.equal(curlCalls.length, 0);
});

test("fetch fallback skips interim 100 Continue blocks and keeps the last status", async () => {
  const { f } = harness({ err: unreachable("EHOSTUNREACH"), stdout: "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 404 Not Found\r\nx-a: b\r\n\r\nnope" });
  const res = await f("http://ollama.lab/v1/x");
  assert.equal(res.status, 404);
  assert.equal(res.statusText, "Not Found");
  assert.equal(res.headers.get("x-a"), "b");
  assert.equal(await res.text(), "nope");
});

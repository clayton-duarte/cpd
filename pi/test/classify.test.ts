import assert from "node:assert/strict";
import { test } from "node:test";
import { main, type ClassifyDeps } from "../lib/classify.ts";

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

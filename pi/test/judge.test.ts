import assert from "node:assert/strict";
import { test } from "node:test";
import { judge, main, type Answer, type JudgeRequest, type JudgeRuntime } from "../lib/judge.ts";

const KEY = "sk-bf-test-key";
const BASE = "http://bifrost.test/v1";
const MODEL = "vertex/gemini-3.8-flash";

const request: JudgeRequest = {
  model: MODEL,
  new: { id: "m1-d2", kind: "decision", at: "2026-11-03T12:00:00Z", text: "Use Postgres for the store." },
  neighbour: { id: "m1-n1", kind: "note", at: "2026-10-01T09:00:00Z", topic: "storage", text: "The store is SQLite." },
};
const label: JudgeRequest = { ...request, neighbour: null };

const reply = (content: unknown, status = 200) =>
  new Response(JSON.stringify({ choices: [{ message: { content: typeof content === "string" ? content : JSON.stringify(content) } }] }), { status });
const verdict = { verdict: "supersedes", quote: "Use Postgres", reason: "newer choice", topic: "storage", short: "Store moves to Postgres" };

const runtime: JudgeRuntime = {
  getProviders: () => [{ id: "bifrost", baseUrl: BASE }],
  getAuth: async () => ({ auth: { apiKey: KEY } }),
};

type Seen = { url: string; init: RequestInit };
/** A fake fetch: records each call and answers `answer`; no socket. */
function fake(answer: () => Promise<Response>): { fetch: typeof fetch; seen: Seen[] } {
  const seen: Seen[] = [];
  const f = (async (url: string | URL | Request, init?: RequestInit) => {
    seen.push({ url: String(url), init: init ?? {} });
    return answer();
  }) as typeof fetch;
  return { fetch: f, seen };
}

const bodyOf = (s: Seen) => JSON.parse(String(s.init.body)) as { model: string; temperature: number; response_format?: unknown; messages: { role: string; content: string }[] };
const promptOf = (s: Seen) => bodyOf(s).messages.map((m) => m.content).join("\n");

test("judge builds a chat request for vertex/gemini-3.8-flash holding both texts", async () => {
  const f = fake(async () => reply(verdict));
  await judge({ rt: runtime, fetch: f.fetch, request });
  assert.equal(f.seen.length, 1);
  assert.equal(f.seen[0]!.url, `${BASE}/chat/completions`);
  const body = bodyOf(f.seen[0]!);
  assert.equal(body.model, MODEL);
  assert.equal(body.temperature, 0);
  assert.ok(body.response_format, "asks for a JSON-only reply");
  const prompt = promptOf(f.seen[0]!);
  assert.ok(prompt.includes(request.new.text));
  assert.ok(prompt.includes(request.neighbour!.text));
  assert.equal((f.seen[0]!.init.headers as Record<string, string>).Authorization, `Bearer ${KEY}`);
});

test("judge prompt defines a partial overrule as contradicts and demands a verbatim quote", async () => {
  const f = fake(async () => reply(verdict));
  await judge({ rt: runtime, fetch: f.fetch, request });
  const prompt = promptOf(f.seen[0]!);
  assert.match(prompt, /partial/i);
  assert.match(prompt, /contradicts/);
  assert.match(prompt, /verbatim/i);
  assert.match(prompt, /quote/i);
});

test("judge label request asks only for topic and short", async () => {
  const f = fake(async () => reply({ topic: "storage", short: "Which store?" }));
  await judge({ rt: runtime, fetch: f.fetch, request: label });
  const prompt = promptOf(f.seen[0]!);
  assert.match(prompt, /topic/);
  assert.match(prompt, /short/);
  assert.doesNotMatch(prompt, /duplicate|supersedes|contradicts|verdict|quote/i);
  assert.ok(!prompt.includes("The store is SQLite."));
});

test("judge prints one JSON line with verdict quote reason topic and short", async () => {
  const f = fake(async () => reply(verdict));
  let out = "";
  const status = await main({ rt: runtime, fetch: f.fetch, stdin: JSON.stringify(request), stdout: { write: (t) => (out += t) }, stderr: { write: () => true } });
  assert.equal(status, 0);
  assert.ok(out.endsWith("\n") && out.trim().split("\n").length === 1);
  assert.deepEqual(JSON.parse(out), { ok: true, ...verdict });
  const labelled = fake(async () => reply({ topic: "storage", short: "Which store?" }));
  out = "";
  await main({ rt: runtime, fetch: labelled.fetch, stdin: JSON.stringify(label), stdout: { write: (t) => (out += t) }, stderr: { write: () => true } });
  assert.deepEqual(JSON.parse(out), { ok: true, verdict: "independent", quote: "", reason: "", topic: "storage", short: "Which store?" });
});

test("judge never prints the api key", async () => {
  const echo = fake(async () => new Response(`denied for ${KEY}`, { status: 401 }));
  const boom = fake(async () => { throw new Error(`connect failed with ${KEY}`); });
  const junk = fake(async () => reply(`not json ${KEY}`));
  for (const f of [echo, boom, junk]) {
    let out = "";
    let err = "";
    await main({ rt: runtime, fetch: f.fetch, stdin: JSON.stringify(request), stdout: { write: (t) => (out += t) }, stderr: { write: (t) => (err += t) } });
    assert.ok(out.length > 0);
    assert.ok(!out.includes(KEY) && !err.includes(KEY));
  }
});

const failure = (a: Answer) => {
  assert.equal(a.ok, false);
  return (a as Extract<Answer, { ok: false }>).error;
};

test("judge maps a rejected fetch to judge_unreachable", async () => {
  const f = fake(async () => { throw new TypeError("fetch failed"); });
  assert.equal(failure(await judge({ rt: runtime, fetch: f.fetch, request })).code, "judge_unreachable");
});

test("judge maps a non-2xx answer to judge_refused with the status", async () => {
  const f = fake(async () => new Response("rate limited", { status: 429 }));
  const error = failure(await judge({ rt: runtime, fetch: f.fetch, request }));
  assert.equal(error.code, "judge_refused");
  assert.equal(error.details.status, 429);
});

test("judge maps a non-JSON model reply to judge_bad_reply", async () => {
  const f = fake(async () => reply("I think they are the same."));
  assert.equal(failure(await judge({ rt: runtime, fetch: f.fetch, request })).code, "judge_bad_reply");
});

test("judge maps an unknown verdict to judge_bad_reply", async () => {
  const f = fake(async () => reply({ ...verdict, verdict: "maybe" }));
  assert.equal(failure(await judge({ rt: runtime, fetch: f.fetch, request })).code, "judge_bad_reply");
});

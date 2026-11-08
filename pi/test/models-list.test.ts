import assert from "node:assert/strict";
import { createServer, type IncomingMessage, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { test } from "node:test";
import { listModels, main, type Answer, type Entry, type Failure, type ListRuntime } from "../lib/models-list.ts";

const KEY = "sk-bf-test-key";
const PI_VERSION = "1.3.0";

type Hit = { method: string; url: string; headers: IncomingMessage["headers"] };
type Handler = (req: IncomingMessage, res: import("node:http").ServerResponse) => void;

/** A local http server standing in for a gateway; `close()` also drops the connections a hanging handler holds. */
async function serve(handler: Handler): Promise<{ origin: string; port: number; hits: Hit[]; close(): Promise<void> }> {
  const hits: Hit[] = [];
  const server: Server = createServer((req, res) => {
    hits.push({ method: req.method ?? "", url: req.url ?? "", headers: req.headers });
    handler(req, res);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = (server.address() as AddressInfo).port;
  return {
    origin: `http://127.0.0.1:${port}`,
    port,
    hits,
    close: () =>
      new Promise<void>((resolve) => {
        server.close(() => resolve());
        server.closeAllConnections();
      }),
  };
}

const json = (body: unknown): Handler => (_req, res) => {
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(body));
};
const models = (...ids: string[]) => json({ object: "", data: ids.map((id) => ({ id, object: "model", owned_by: "" })) });

/** A port nothing listens on: bound, then released. */
async function closedPort(): Promise<number> {
  const s = await serve(() => {});
  const { port } = s;
  await s.close();
  return port;
}

type Fake = {
  /** Built-in providers have a `refreshModels`; custom ones (models.json) have none. */
  custom?: Record<string, { baseUrl?: string; key?: string | "throws" | "none" }>;
  /** A built-in provider's `--all` listing: `auth` `"none"` (or absent) leaves it out, an empty `available` too. */
  builtin?: Record<string, { baseUrl?: string; auth?: "oauth" | "api_key" | "none"; available?: string[]; known?: string[] }>;
  copilot?: { available: string[]; known: string[]; auth: "oauth" | "api_key" | "none"; errors?: string[] };
};

/** A fake pi runtime; the methods are also under `models`, where the research documents them (`rt.models.getAvailable`). */
function runtime(fake: Fake): { rt: ListRuntime; refreshes: unknown[]; authAsked: string[] } {
  const refreshes: unknown[] = [];
  const authAsked: string[] = [];
  const providers = [
    ...Object.entries(fake.builtin ?? {}).map(([id, p]) => ({ id, baseUrl: p.baseUrl, refreshModels: () => Promise.resolve() })),
    ...(fake.copilot ? [{ id: "github-copilot", baseUrl: "https://api.individual.githubcopilot.com", refreshModels: () => Promise.resolve() }] : []),
    ...Object.entries(fake.custom ?? {}).map(([id, p]) => ({ id, baseUrl: p.baseUrl })),
  ];
  const methods = {
    getProviders: () => providers,
    getAuth: async (p: string | { provider: string }) => {
      const id = typeof p === "string" ? p : p.provider;
      authAsked.push(id);
      const key = fake.custom?.[id]?.key;
      if (key === "throws") throw Object.assign(new Error(`API key auth failed for provider ${id}: Failed to resolve API key from environment variable`), { code: "auth" });
      if (key === "none") return undefined;
      return { auth: { apiKey: key ?? KEY } };
    },
    getAllModels: (p?: string) => {
      const id = typeof p === "string" ? p : p;
      if (id === "github-copilot") return (fake.copilot?.known ?? []).map((id) => ({ id }));
      return (fake.builtin?.[id ?? ""]?.known ?? []).map((id) => ({ id }));
    },
    getAvailable: async (p?: string) => {
      const id = typeof p === "string" ? p : p;
      if (id === "github-copilot") return (fake.copilot && fake.copilot.auth !== "none" ? fake.copilot.available : []).map((id) => ({ id }));
      const b = fake.builtin?.[id ?? ""];
      return (b && b.auth !== "none" ? b.available ?? [] : []).map((id) => ({ id }));
    },
    checkAuth: async (p?: string) => {
      const id = typeof p === "string" ? p : p;
      if (id === "github-copilot") return fake.copilot && fake.copilot.auth !== "none" ? { type: fake.copilot.auth } : undefined;
      const b = fake.builtin?.[id ?? ""];
      return b && b.auth && b.auth !== "none" ? { type: b.auth } : undefined;
    },
    refresh: async (options: unknown) => {
      refreshes.push(options);
      return { aborted: false, errors: new Map((fake.copilot?.errors ?? []).map((m, i) => [`github-copilot${i}`, new Error(m)])) };
    },
  };
  const rt = { ...methods, models: methods } as unknown as ListRuntime;
  return { rt, refreshes, authAsked };
}

const entry = (answer: Answer, provider: string): Entry => {
  assert.ok(answer.ok, JSON.stringify(answer));
  const found = answer.providers.find((e) => e.provider === provider);
  assert.ok(found, `no entry for ${provider}: ${JSON.stringify(answer)}`);
  return found;
};
const listed = (e: Entry) => {
  assert.ok("ids" in e, `the provider failed: ${JSON.stringify(e)}`);
  return e;
};
/** The failure of a provider: its own entry's, or the whole run's (an answer that holds no provider but the failure). */
const failure = (answer: Answer, provider: string): Failure => {
  if (!answer.ok) return answer.error;
  const e = entry(answer, provider);
  assert.ok("error" in e, `the provider did not fail: ${JSON.stringify(e)}`);
  return e.error;
};

test("custom provider list: the ids come from GET <baseUrl>/models with the provider's key as a Bearer header", async () => {
  const gateway = await serve(models("vertex/claude-opus-5", "vertex/claude-haiku-4-5", "vertex/gemini-3.8-flash"));
  try {
    const { rt } = runtime({ custom: { bifrost: { baseUrl: `${gateway.origin}/openai/v1` } } });
    const answer = await listModels({ rt, piVersion: PI_VERSION });

    assert.ok(answer.ok);
    assert.equal(answer.pi_version, PI_VERSION);
    const bifrost = listed(entry(answer, "bifrost"));
    assert.equal(bifrost.auth, "custom");
    assert.equal(bifrost.base, gateway.origin, "base is the origin: no path, no query");
    assert.deepEqual([...bifrost.ids].sort(), ["vertex/claude-haiku-4-5", "vertex/claude-opus-5", "vertex/gemini-3.8-flash"]);

    assert.equal(gateway.hits.length, 1);
    assert.equal(gateway.hits[0]!.method, "GET");
    assert.equal(gateway.hits[0]!.url, "/openai/v1/models");
    assert.equal(gateway.hits[0]!.headers.authorization, `Bearer ${KEY}`);
    assert.deepEqual(Object.keys(gateway.hits[0]!.headers).filter((h) => h.startsWith("x-") || h === "cookie" || h === "content-type"), [], "no header of its own beyond Authorization");
  } finally {
    await gateway.close();
  }
});

test("custom provider list: an unset key is models_key_unset and nothing is fetched", async () => {
  const gateway = await serve(models("a"));
  try {
    const { rt } = runtime({
      custom: {
        thrown: { baseUrl: `${gateway.origin}/v1`, key: "throws" },
        nothing: { baseUrl: `${gateway.origin}/v1`, key: "none" },
      },
    });
    const answer = await listModels({ rt, piVersion: PI_VERSION });

    for (const provider of ["thrown", "nothing"]) assert.equal(failure(answer, provider).code, "models_key_unset", provider);
    assert.equal(gateway.hits.length, 0, "no request is made without a key");
  } finally {
    await gateway.close();
  }
});

test("custom provider list: 401, a refused connection, a TLS error and a timeout are told apart", async () => {
  const unauthorized = await serve((_req, res) => {
    res.statusCode = 401;
    res.end(JSON.stringify({ error: { message: "access not found" } }));
  });
  const broken = await serve((_req, res) => {
    res.statusCode = 503;
    res.end("down");
  });
  const silent = await serve(() => {});
  const refusedPort = await closedPort();
  try {
    const realFetch = globalThis.fetch;
    // A TLS failure needs a certificate nobody trusts: the fake `fetch` fails as Node's does (docs/research D1).
    const fakeFetch: typeof fetch = (input, init) => {
      if (String(input).startsWith("https://tls.invalid")) {
        throw Object.assign(new TypeError("fetch failed"), { cause: Object.assign(new Error("unable to get local issuer certificate"), { code: "UNABLE_TO_GET_ISSUER_CERT_LOCALLY" }) });
      }
      return realFetch(input, init);
    };
    const { rt } = runtime({
      custom: {
        denied: { baseUrl: `${unauthorized.origin}/v1` },
        refused: { baseUrl: `http://127.0.0.1:${refusedPort}/v1` },
        tls: { baseUrl: "https://tls.invalid/v1" },
        stalled: { baseUrl: `${silent.origin}/v1` },
        down: { baseUrl: `${broken.origin}/v1` },
      },
    });
    const answer = await listModels({ rt, piVersion: PI_VERSION, fetch: fakeFetch, fetchTimeoutMs: 300 });

    assert.equal(failure(answer, "denied").code, "models_unauthorized");
    assert.equal(failure(answer, "refused").code, "models_unreachable");
    assert.equal(failure(answer, "tls").code, "models_tls");
    assert.equal(failure(answer, "stalled").code, "models_timeout");
    const down = failure(answer, "down");
    assert.equal(down.code, "models_unreachable", "any other HTTP status is unreachable");
    assert.equal(down.details.status, 503);
  } finally {
    await Promise.all([unauthorized.close(), broken.close(), silent.close()]);
  }
});

test("custom provider list: an empty or malformed data array is an error, never an empty list", async () => {
  const cases: Record<string, { handler: Handler; code: string }> = {
    emptydata: { handler: json({ object: "", data: [] }), code: "models_empty" },
    notjson: { handler: (_req, res) => void res.end("<html>gateway</html>"), code: "models_unparseable" },
    nodata: { handler: json({ object: "" }), code: "models_unparseable" },
    datanotarray: { handler: json({ data: "x" }), code: "models_unparseable" },
    noid: { handler: json({ data: [{ name: "x" }] }), code: "models_unparseable" },
    emptyid: { handler: json({ data: [{ id: "" }] }), code: "models_unparseable" },
    unsafe: { handler: json({ data: [{ id: "a" }] }), code: "models_unparseable" },
  };
  const gateway = await serve((req, res) => {
    const name = (req.url ?? "").split("/")[1] ?? "";
    (cases[name] ?? cases.unsafe!).handler(req, res);
  });
  try {
    const custom: Fake["custom"] = {};
    for (const name of Object.keys(cases)) custom[name] = { baseUrl: `${gateway.origin}/${name}` };
    // A provider name that is not a file-name-safe token is unparseable, whatever its list holds.
    delete custom.unsafe;
    custom["../evil"] = { baseUrl: `${gateway.origin}/unsafe` };
    const { rt } = runtime({ custom });
    const answer = await listModels({ rt, piVersion: PI_VERSION });

    for (const [name, { code }] of Object.entries(cases)) {
      if (name === "unsafe") continue;
      assert.equal(failure(answer, name).code, code, name);
      assert.ok(!("ids" in entry(answer, name)), `${name} holds no ids`);
    }
    assert.equal(failure(answer, "../evil").code, "models_unparseable");
  } finally {
    await gateway.close();
  }
});

test("the helper never prints the key", async () => {
  // The gateway echoes the Authorization header back, in its 401 and in its 200: the worst case for a leak.
  const echo = await serve((req, res) => {
    const seen = String(req.headers.authorization);
    if ((req.url ?? "").startsWith("/bad")) {
      res.statusCode = 401;
      res.end(JSON.stringify({ error: { message: `credential ${seen} revoked` } }));
    } else {
      res.end(JSON.stringify({ data: [{ id: "m1", owned_by: seen }] }));
    }
  });
  try {
    const { rt } = runtime({
      custom: { good: { baseUrl: `${echo.origin}/good` }, bad: { baseUrl: `${echo.origin}/bad` }, unset: { baseUrl: `${echo.origin}/x`, key: "throws" } },
      copilot: { available: ["gpt-5.6-sol"], known: ["gpt-5.6-sol"], auth: "oauth" },
    });
    const out: string[] = [];
    const err: string[] = [];
    const status = await main({ rt, piVersion: PI_VERSION, stdout: { write: (t) => out.push(t) }, stderr: { write: (t) => err.push(t) } });

    assert.equal(status, 0);
    const stdout = out.join("");
    assert.equal(stdout.trimEnd().split("\n").length, 1, "ONE JSON value on stdout");
    const answer = JSON.parse(stdout) as Answer;
    assert.ok(answer.ok);
    assert.equal(answer.providers.length, 4);
    assert.equal(stdout.includes(KEY), false, "stdout");
    assert.equal(err.join("").includes(KEY), false, "stderr");
    assert.equal(JSON.stringify(await listModels({ rt, piVersion: PI_VERSION })).includes(KEY), false, "the answer, details included");
  } finally {
    await echo.close();
  }
});

test("github-copilot list: the ids are the available models and the auth type is recorded", async () => {
  const known = ["claude-haiku-4.5", "claude-opus-5", "gpt-5.6-sol", "gpt-6-astra", "grok-4.5"];
  const available = ["gpt-5.6-sol", "claude-opus-5"];
  for (const auth of ["oauth", "api_key"] as const) {
    const { rt, refreshes } = runtime({ copilot: { available, known, auth } });
    const answer = await listModels({ rt, piVersion: PI_VERSION });

    const copilot = listed(entry(answer, "github-copilot"));
    assert.equal(copilot.auth, auth);
    assert.equal(copilot.base, null);
    assert.deepEqual([...copilot.ids].sort(), ["claude-opus-5", "gpt-5.6-sol"], "the account's list, not pi's compiled one");
    assert.deepEqual([...(copilot.known ?? [])].sort(), [...known].sort());
    assert.deepEqual(refreshes, [{ allowNetwork: true, force: true, providers: ["github-copilot"] }]);
  }
});

test("github-copilot list: no login leaves it out of the answer", async () => {
  const gateway = await serve(models("m1"));
  try {
    const { rt } = runtime({ custom: { bifrost: { baseUrl: `${gateway.origin}/v1` } }, copilot: { available: [], known: ["gpt-5.6-sol"], auth: "none" } });
    const answer = await listModels({ rt, piVersion: PI_VERSION });
    assert.ok(answer.ok);
    assert.deepEqual(answer.providers.map((e) => e.provider), ["bifrost"]);

    // pi can use no provider the rule lists: the whole run fails.
    const none = await listModels({ rt: runtime({ copilot: { available: [], known: [], auth: "none" }, builtin: { anthropic: { baseUrl: "https://api.anthropic.com" } } }).rt, piVersion: PI_VERSION });
    assert.equal(none.ok, false);
    assert.equal(failure(none, "").code, "models_no_provider");
  } finally {
    await gateway.close();
  }
});

test("github-copilot list: a refresh error that pi swallowed is raised", async () => {
  const { rt } = runtime({ copilot: { available: ["gpt-5.6-sol"], known: ["gpt-5.6-sol"], auth: "oauth", errors: ["pi.dev catalog: 500", "second"] } });
  const answer = await listModels({ rt, piVersion: PI_VERSION });

  const failed = failure(answer, "github-copilot");
  assert.equal(failed.code, "models_refresh_failed");
  assert.equal(failed.message.includes("pi.dev catalog: 500"), true, "the first error is the message");
  if (answer.ok) assert.equal("ids" in entry(answer, "github-copilot"), false, "no list of a failed refresh");
});

test("the providers are github-copilot (when logged in) and the custom providers of models.json with a baseUrl, never a built-in one; one failing provider does not hide the others", async () => {
  const bifrost = await serve(models("vertex/claude-opus-5"));
  const broken = await serve((_req, res) => {
    res.statusCode = 401;
    res.end("no");
  });
  const builtin = await serve(models("must-not-be-listed"));
  try {
    const { rt } = runtime({
      builtin: { anthropic: { baseUrl: builtin.origin }, openai: { baseUrl: builtin.origin } },
      custom: {
        bifrost: { baseUrl: `${bifrost.origin}/openai/v1` },
        rejected: { baseUrl: `${broken.origin}/v1` },
        nobase: {},
      },
      copilot: { available: ["gpt-5.6-sol"], known: ["gpt-5.6-sol", "gpt-6-astra"], auth: "oauth" },
    });
    const answer = await listModels({ rt, piVersion: PI_VERSION });

    assert.ok(answer.ok);
    assert.deepEqual(answer.providers.map((e) => e.provider).sort(), ["bifrost", "github-copilot", "rejected"]);
    assert.deepEqual(listed(entry(answer, "bifrost")).ids, ["vertex/claude-opus-5"]);
    assert.deepEqual(listed(entry(answer, "github-copilot")).ids, ["gpt-5.6-sol"]);
    assert.equal(failure(answer, "rejected").code, "models_unauthorized");
    assert.equal(builtin.hits.length, 0, "a built-in provider is never fetched");
  } finally {
    await Promise.all([bifrost.close(), broken.close(), builtin.close()]);
  }
});

test("--all lists the built-in providers pi has auth for and models to offer, beside github-copilot; the others are left out, and plain mode never lists one", async () => {
  const { rt } = runtime({
    builtin: {
      anthropic: { auth: "oauth", available: ["claude-sonnet-5.5"], known: ["claude-sonnet-5.5", "claude-opus-5.5"] },
      openai: { auth: "none", available: ["gpt-5.6-sol"] }, // no login: left out
      gemini: { auth: "api_key", available: [] }, // a login, but no offered model: left out
    },
    copilot: { available: ["gpt-5.6-sol"], known: ["gpt-5.6-sol"], auth: "oauth" },
  });

  const all = await listModels({ rt, piVersion: PI_VERSION, all: true });
  assert.ok(all.ok);
  assert.deepEqual(all.providers.map((e) => e.provider).sort(), ["anthropic", "github-copilot"], JSON.stringify(all));
  const anthropic = listed(entry(all, "anthropic"));
  assert.equal(anthropic.auth, "oauth");
  assert.equal(anthropic.base, null);
  assert.deepEqual(anthropic.ids, ["claude-sonnet-5.5"]);
  assert.deepEqual(anthropic.known, ["claude-sonnet-5.5", "claude-opus-5.5"]);

  // Without `--all` the built-ins are never listed, whatever pi has for them.
  const plain = await listModels({ rt, piVersion: PI_VERSION });
  assert.ok(plain.ok);
  assert.deepEqual(plain.providers.map((e) => e.provider), ["github-copilot"]);
});

test("--all: a built-in whose auth check throws is one failed entry, never hiding the others", async () => {
  const rt = runtime({ builtin: { anthropic: { auth: "oauth", available: ["m1"] } } }).rt;
  rt.checkAuth = async () => {
    throw new Error("pi's runtime exploded");
  };
  const answer = await listModels({ rt, piVersion: PI_VERSION, all: true });
  assert.equal(failure(answer, "anthropic").code, "models_refresh_failed");
});

test("a custom provider whose key pi cannot resolve is skipped", async () => {
  const gateway = await serve(models("m1"));
  try {
    const { rt } = runtime({
      custom: {
        // getAuth throws for an unset key pi cannot resolve.
        thrown: { baseUrl: `${gateway.origin}/v1`, key: "throws" },
        // getAuth answers nothing: the provider has no key at all.
        nothing: { baseUrl: `${gateway.origin}/v1`, key: "none" },
        // A provider pi does have a key for is still listed.
        keyed: { baseUrl: `${gateway.origin}/v1` },
      },
    });
    const answer = await listModels({ rt, piVersion: PI_VERSION });

    assert.ok(answer.ok, JSON.stringify(answer));
    // A provider pi cannot resolve a key for is skipped, not a failed entry: it never appears in the answer.
    assert.deepEqual(answer.providers.map((e) => e.provider), ["keyed"], JSON.stringify(answer));
    assert.equal(answer.providers.some((e) => "error" in e), false, "no failed entry");
    assert.equal(JSON.stringify(answer).includes("models_key_unset"), false, "no models_key_unset");
    // The provider pi has a key for still appears with its ids.
    const keyed = listed(entry(answer, "keyed"));
    assert.equal(keyed.auth, "custom");
    assert.deepEqual(keyed.ids, ["m1"]);
    assert.equal(gateway.hits.length, 1, "only the provider with a key is fetched");
  } finally {
    await gateway.close();
  }
});

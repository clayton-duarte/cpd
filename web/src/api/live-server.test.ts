// Dev live-server route tests for the dev live adapter (web/README.md "The `/__cpd/` routes"
// and "`web/src/api/live-server.ts`", hs-d819). RED: `createLiveHandler` still throws
// `not implemented`, so these fail now and pass once the impl lands.
//
// Every external boundary is the injected `LiveDeps`: no test spawns cpd, opens a socket, starts
// a server or touches the network. The fake request is read the way Node reads an
// `IncomingMessage` — as an async iterable of Buffers — and the fake response captures
// status/headers/body and can emit `close`, so the SSE loop stops.
import { readFileSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createLiveHandler, type LiveDeps, type LiveHandler, type LiveSession } from "./live-server";
import type { SnapshotFrame } from "../fold/fold";

const TOKEN = "test-token";
const originalToken = process.env.CPD_WEB_TOKEN;

beforeEach(() => {
  process.env.CPD_WEB_TOKEN = TOKEN;
});

afterEach(() => {
  if (originalToken === undefined) delete process.env.CPD_WEB_TOKEN;
  else process.env.CPD_WEB_TOKEN = originalToken;
});

const piSession: LiveSession = { id: "s-pi", role: "lead", runner: "pi", title: "pi lead" };
const durableSession: LiveSession = {
  id: "s-durable",
  role: "manager",
  runner: "durable",
  title: "durable manager",
};

/** Reads the committed pi fixture beside this test. */
function piJsonl(): string {
  return readFileSync(new URL("./pi-session.fixture.jsonl", import.meta.url), "utf8");
}

/** One pi session-file line carrying a single user text. */
function piLine(text: string): string {
  return JSON.stringify({
    type: "message",
    id: "m1",
    message: { role: "user", content: [{ type: "text", text }] },
  });
}

/** The deps with every boundary a harmless default; each test overrides what it exercises. */
function deps(overrides: Partial<LiveDeps> = {}): LiveDeps {
  return {
    list: async () => [],
    readPi: async () => undefined,
    tail: async () => "",
    send: async () => {},
    stat: async () => undefined,
    ...overrides,
  };
}

interface ReqOptions {
  method: string;
  url: string;
  headers?: Record<string, string>;
  body?: string;
}

/** A Node-shaped request: method/url/lowercased headers, and the body as an async iterable. */
function makeReq(opts: ReqOptions): IncomingMessage {
  const chunks = opts.body === undefined ? [] : [Buffer.from(opts.body)];
  const headers: Record<string, string> = {};
  for (const [name, value] of Object.entries(opts.headers ?? {})) {
    headers[name.toLowerCase()] = value;
  }
  const req = {
    method: opts.method,
    url: opts.url,
    headers,
    async *[Symbol.asyncIterator]() {
      for (const chunk of chunks) yield chunk;
    },
  };
  return req as unknown as IncomingMessage;
}

interface ResCapture {
  res: ServerResponse;
  status(): number;
  header(name: string): string | undefined;
  body(): string;
  /** Emits the `close` event the SSE loop stops on. */
  close(): void;
}

/** A Node-shaped response capturing status/headers/body, with a controllable `close`. */
function makeRes(): ResCapture {
  const headers: Record<string, string> = {};
  let statusCode = 200;
  let body = "";
  const listeners: Record<string, Array<() => void>> = {};
  const res: Record<string, unknown> = {};
  Object.defineProperty(res, "statusCode", {
    get: () => statusCode,
    set: (value: number) => {
      statusCode = value;
    },
    enumerable: true,
  });
  res.writeHead = (code: number, hdrs?: Record<string, string>) => {
    statusCode = code;
    for (const [name, value] of Object.entries(hdrs ?? {})) headers[name.toLowerCase()] = String(value);
    return res;
  };
  res.setHeader = (name: string, value: string) => {
    headers[name.toLowerCase()] = String(value);
    return res;
  };
  res.write = (chunk: string | Uint8Array) => {
    body += typeof chunk === "string" ? chunk : Buffer.from(chunk).toString("utf8");
    return true;
  };
  res.end = (chunk?: string | Uint8Array) => {
    if (chunk !== undefined) {
      body += typeof chunk === "string" ? chunk : Buffer.from(chunk).toString("utf8");
    }
    return res;
  };
  res.on = (event: string, callback: () => void) => {
    (listeners[event] ??= []).push(callback);
    return res;
  };
  res.once = res.on;
  res.removeListener = () => res;
  return {
    res: res as unknown as ServerResponse,
    status: () => statusCode,
    header: (name) => headers[name.toLowerCase()],
    body: () => body,
    close: () => {
      for (const callback of listeners["close"] ?? []) callback();
    },
  };
}

/** Sends one request through the handler and captures the response. */
async function call(
  handler: LiveHandler,
  opts: ReqOptions,
): Promise<{ matched: boolean; res: ResCapture }> {
  const res = makeRes();
  const matched = await handler(makeReq(opts), res.res);
  return { matched, res };
}

/** Flushes pending microtasks (the handler's awaits). */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
}

/** Parses the SSE `data:` events of a response body. */
function frames(body: string): SnapshotFrame[] {
  return body
    .split("\n\n")
    .filter((block) => block.startsWith("data:"))
    .map((block) => JSON.parse(block.slice("data:".length).trim()) as SnapshotFrame);
}

/** The text of a snapshot frame's first message. */
function snapshotText(frame: SnapshotFrame): string {
  const snapshot = frame.snapshot as { messages?: Array<{ text: string }> };
  return snapshot.messages?.[0]?.text ?? "";
}

test("the handler refuses a request without the token", async () => {
  const handler = createLiveHandler(deps());
  const missing = await call(handler, { method: "GET", url: "/__cpd/sessions" });
  expect(missing.matched).toBe(true);
  expect(missing.res.status()).toBe(401);
  expect(JSON.parse(missing.res.body())).toEqual({ ok: false, error: "missing or wrong token" });

  const wrong = await call(handler, {
    method: "GET",
    url: "/__cpd/sessions",
    headers: { "x-cpd-token": "not-the-token" },
  });
  expect(wrong.res.status()).toBe(401);
});

test("the handler lists lead and manager sessions", async () => {
  const handler = createLiveHandler(deps({ list: async () => [piSession, durableSession] }));
  const { matched, res } = await call(handler, {
    method: "GET",
    url: "/__cpd/sessions",
    headers: { "x-cpd-token": TOKEN },
  });
  expect(matched).toBe(true);
  expect(res.status()).toBe(200);
  expect(JSON.parse(res.body())).toEqual({ ok: true, sessions: [piSession, durableSession] });
});

test("the handler serves a pi session transcript from its session file", async () => {
  const jsonl = piJsonl();
  const handler = createLiveHandler(
    deps({
      list: async () => [piSession],
      readPi: async (id) => (id === piSession.id ? jsonl : undefined),
    }),
  );
  const { matched, res } = await call(handler, {
    method: "GET",
    url: `/__cpd/sessions/${piSession.id}/transcript`,
    headers: { "x-cpd-token": TOKEN },
  });
  expect(matched).toBe(true);
  expect(res.status()).toBe(200);
  const payload = JSON.parse(res.body()) as {
    ok: boolean;
    session: string;
    messages: Array<{ role: string; text: string }>;
  };
  expect(payload.ok).toBe(true);
  expect(payload.session).toBe(piSession.id);
  expect(payload.messages.map((m) => m.role)).toEqual(["user", "assistant", "assistant"]);
  expect(payload.messages[0]!.text).toBe("ping");
});

test("the SSE route streams a snapshot frame and again when the transcript changes", async () => {
  vi.useFakeTimers();
  try {
    let mtime = 1;
    let jsonl = piLine("hello");
    const handler = createLiveHandler(
      deps({
        list: async () => [piSession],
        readPi: async () => jsonl,
        stat: async () => mtime,
      }),
    );

    const capture = makeRes();
    const done = handler(
      makeReq({
        method: "GET",
        url: `/__cpd/sessions/${piSession.id}/frames`,
        headers: { "x-cpd-token": TOKEN },
      }),
      capture.res,
    );
    await flush();
    await vi.advanceTimersByTimeAsync(0);
    await flush();

    // The current snapshot at once: epoch "live", seq 1 (web/README.md, the routes table).
    const first = frames(capture.body());
    expect(first).toHaveLength(1);
    expect(first[0]!.epoch).toBe("live");
    expect(first[0]!.seq).toBe(1);
    expect(snapshotText(first[0]!)).toBe("hello");

    // The transcript changes (a new mtime): a re-sent snapshot at seq + 1.
    jsonl = piLine("world");
    mtime = 2;
    await vi.advanceTimersByTimeAsync(5000);
    await flush();

    const all = frames(capture.body());
    expect(all.length).toBeGreaterThanOrEqual(2);
    expect(all[1]!.seq).toBe(2);
    expect(snapshotText(all[1]!)).toBe("world");

    // The client closing ends the stream.
    capture.close();
    await done;
  } finally {
    vi.useRealTimers();
  }
});

test("the send route runs cpd session send with the unblocks reason", async () => {
  const sent: Array<[string, string, string]> = [];
  const handler = createLiveHandler(
    deps({
      list: async () => [piSession],
      send: async (id, text, unblocks) => {
        sent.push([id, text, unblocks]);
      },
    }),
  );
  // The token also travels in the query parameter (the route accepts either).
  const { matched, res } = await call(handler, {
    method: "POST",
    url: `/__cpd/sessions/${piSession.id}/send?token=${TOKEN}`,
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ text: "please resume", unblocks: "the user answered" }),
  });
  expect(matched).toBe(true);
  expect(res.status()).toBe(200);
  expect(JSON.parse(res.body())).toEqual({ ok: true });
  expect(sent).toEqual([[piSession.id, "please resume", "the user answered"]]);
});

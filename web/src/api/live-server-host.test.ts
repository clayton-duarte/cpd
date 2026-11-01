// Dev live-server host-socket tests of the dev live adapter (web/README.md "The durable host-socket
// read", hs-d819/chat-host). RED: the live-server's durable-read branch still throws `not implemented`,
// so these fail now and pass once the impl lands.
//
// Every external boundary is the injected `LiveDeps`: no test spawns cpd, opens a socket, starts a
// server or touches the network. The fake request/response are Node-shaped (an async iterable body,
// a response capturing status/headers/body), the same style as web/src/api/live-server.test.ts.
import type { IncomingMessage, ServerResponse } from "node:http";
import { afterEach, beforeEach, expect, test } from "vitest";
import { createLiveHandler, type LiveDeps, type LiveHandler, type LiveSession } from "./live-server";
import type { TranscriptMessage } from "../transcript";

const TOKEN = "test-token";
const originalToken = process.env.CPD_WEB_TOKEN;

beforeEach(() => {
  process.env.CPD_WEB_TOKEN = TOKEN;
});

afterEach(() => {
  if (originalToken === undefined) delete process.env.CPD_WEB_TOKEN;
  else process.env.CPD_WEB_TOKEN = originalToken;
});

const durableSession: LiveSession = {
  id: "s-durable",
  role: "manager",
  runner: "durable",
  title: "durable manager",
};

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
}

/** A Node-shaped request: method/url/lowercased headers and an empty async-iterable body. */
function makeReq(opts: ReqOptions): IncomingMessage {
  const headers: Record<string, string> = {};
  for (const [name, value] of Object.entries(opts.headers ?? {})) {
    headers[name.toLowerCase()] = value;
  }
  const req = {
    method: opts.method,
    url: opts.url,
    headers,
    async *[Symbol.asyncIterator]() {
      // no body
    },
  };
  return req as unknown as IncomingMessage;
}

interface ResCapture {
  res: ServerResponse;
  status(): number;
  body(): string;
}

/** A Node-shaped response capturing status/headers/body (no streaming in these tests). */
function makeRes(): ResCapture {
  const headers: Record<string, string> = {};
  let statusCode = 200;
  let body = "";
  const res: Record<string, unknown> = {};
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
  res.on = () => res;
  res.once = () => res;
  res.removeListener = () => res;
  return { res: res as unknown as ServerResponse, status: () => statusCode, body: () => body };
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

function transcriptUrl(id: string): string {
  return `/__cpd/sessions/${id}/transcript`;
}

test("the server serves a durable Session from its host socket", async () => {
  const hostCalls: string[] = [];
  let tailed = false;
  const hostMessages: TranscriptMessage[] = [
    { id: "host-1", role: "user", text: "run the checks" },
    { id: "host-2", role: "assistant", text: "on it" },
  ];
  const handler = createLiveHandler(
    deps({
      list: async () => [durableSession],
      host: async (id) => {
        hostCalls.push(id);
        return hostMessages;
      },
      tail: async () => {
        tailed = true;
        return "user: from the tail\n";
      },
    }),
  );

  const { matched, res } = await call(handler, {
    method: "GET",
    url: transcriptUrl(durableSession.id),
    headers: { "x-cpd-token": TOKEN },
  });

  // A live host answers: the transcript is the host read, never `cpd session tail`.
  expect(matched).toBe(true);
  expect(res.status()).toBe(200);
  expect(JSON.parse(res.body())).toEqual({
    ok: true,
    session: durableSession.id,
    messages: hostMessages,
  });
  expect(hostCalls).toEqual([durableSession.id]);
  expect(tailed).toBe(false);
});

test("the server falls back to cpd session tail when no host socket exists", async () => {
  const hostCalls: string[] = [];
  let tailed = false;
  const handler = createLiveHandler(
    deps({
      list: async () => [durableSession],
      // No live host answers (no socket file, a refused connect): the read resolves `undefined`.
      host: async (id) => {
        hostCalls.push(id);
        return undefined;
      },
      tail: async () => {
        tailed = true;
        return "user: run the checks\nassistant: ok, done\n";
      },
    }),
  );

  const { matched, res } = await call(handler, {
    method: "GET",
    url: transcriptUrl(durableSession.id),
    headers: { "x-cpd-token": TOKEN },
  });

  // The host is preferred but does not answer, so the fallback prints the tail transcript.
  expect(matched).toBe(true);
  expect(res.status()).toBe(200);
  const payload = JSON.parse(res.body()) as {
    ok: boolean;
    messages: Array<{ role: string; text: string }>;
  };
  expect(payload.messages.map((message) => [message.role, message.text])).toEqual([
    ["user", "run the checks"],
    ["assistant", "ok, done"],
  ]);
  expect(hostCalls).toEqual([durableSession.id]);
  expect(tailed).toBe(true);
});

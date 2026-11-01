// Browser live-API tests for the dev live adapter (web/README.md "`web/src/api/live.ts`",
// hs-d819). RED: `createLiveApi` still throws `not implemented`, so these fail now and pass
// once the impl lands. `fetch` and the EventSource factory are injected fakes: no test opens a
// real EventSource or touches the network.
import { expect, test } from "vitest";
import { createLiveApi } from "./live";
import type { LiveSession } from "./live-server";
import type { SnapshotFrame } from "../fold/fold";

interface Call {
  url: string;
  init?: RequestInit;
}

/** A fake `fetch` answering one JSON payload and recording every call. */
function jsonFetch(payload: unknown, calls: Call[]): typeof fetch {
  return (async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(input), init });
    return new Response(JSON.stringify(payload), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }) as typeof fetch;
}

/** A fake EventSource: records instances, lets the test emit a `message` and sees `close`. */
class FakeEventSource {
  static instances: FakeEventSource[] = [];
  readonly url: string;
  closed = false;
  onmessage: ((event: MessageEvent) => void) | null = null;
  private readonly listeners: Record<string, Array<(event: MessageEvent) => void>> = {};

  constructor(url: string) {
    this.url = url;
    FakeEventSource.instances.push(this);
  }

  addEventListener(type: string, callback: (event: MessageEvent) => void): void {
    (this.listeners[type] ??= []).push(callback);
  }

  removeEventListener(type: string, callback: (event: MessageEvent) => void): void {
    this.listeners[type] = (this.listeners[type] ?? []).filter((cb) => cb !== callback);
  }

  close(): void {
    this.closed = true;
  }

  /** Emits a `message` event with `data` to both listener styles. */
  emit(data: string): void {
    const event = { data } as MessageEvent;
    this.onmessage?.(event);
    for (const callback of this.listeners["message"] ?? []) callback(event);
  }
}

const FakeSource = FakeEventSource as unknown as typeof EventSource;

test("createLiveApi lists sessions from /__cpd/sessions with the token", async () => {
  const calls: Call[] = [];
  const sessions: readonly LiveSession[] = [
    { id: "s1", role: "lead", runner: "pi", title: "one" },
    { id: "s2", role: "manager", runner: "durable", title: "two" },
  ];
  const api = createLiveApi({
    token: "t0k",
    fetch: jsonFetch({ ok: true, sessions }, calls),
    EventSource: FakeSource,
  });

  await expect(api.sessions()).resolves.toEqual(sessions);
  expect(calls).toHaveLength(1);
  expect(calls[0]!.url).toBe("/__cpd/sessions");
  expect(new Headers(calls[0]!.init?.headers).get("x-cpd-token")).toBe("t0k");
});

test("createLiveApi subscribe opens an EventSource and yields parsed snapshot frames", () => {
  FakeEventSource.instances.length = 0;
  const api = createLiveApi({
    token: "t0k",
    fetch: jsonFetch({ ok: true, sessions: [] }, []),
    EventSource: FakeSource,
  });

  const frames: SnapshotFrame[] = [];
  const close = api.subscribe("s1", (frame) => frames.push(frame));
  const source = FakeEventSource.instances[0]!;
  // An EventSource cannot set headers, so the token travels in the query.
  expect(source.url).toBe("/__cpd/sessions/s1/frames?token=t0k");

  const frame: SnapshotFrame = { type: "snapshot", epoch: "live", seq: 1, snapshot: { messages: [] } };
  source.emit(JSON.stringify(frame));
  expect(frames).toEqual([frame]);

  close();
  expect(source.closed).toBe(true);
});

/** The paged transcript call, under either name an implementation may expose it. */
interface PagedResult {
  messages: readonly unknown[];
  more: boolean;
}

test("history() asks the transcript route for the page above the oldest id it was given", async () => {
  const calls: Call[] = [];
  const messages = [{ id: "m5", role: "assistant", text: "five" }];
  const api = createLiveApi({
    token: "t0k",
    fetch: jsonFetch({ ok: true, messages, more: true }, calls),
    EventSource: FakeSource,
  }) as unknown as {
    transcript?: (id: string, page?: { limit?: number; before?: string }) => Promise<PagedResult>;
    history?: (id: string, before?: string, limit?: number) => Promise<PagedResult>;
  };

  // One of the two methods the contract names for a page of older turns must exist.
  const paged = api.transcript ?? api.history;
  expect(typeof paged).toBe("function");

  const page = api.transcript
    ? await api.transcript("s1", { limit: 5, before: "m8" })
    : await api.history!("s1", "m8", 5);

  expect(page).toEqual({ messages, more: true });
  expect(calls).toHaveLength(1);
  expect(calls[0]!.url).toContain("/__cpd/sessions/s1/transcript");
  // `before` is the oldest id already held; `limit` is how many precede it are asked for.
  expect(calls[0]!.url).toContain("before=m8");
  expect(calls[0]!.url).toContain("limit=5");
  expect(new Headers(calls[0]!.init?.headers).get("x-cpd-token")).toBe("t0k");
});

test("createLiveApi send posts the message to the send route", async () => {
  const calls: Call[] = [];
  const api = createLiveApi({
    token: "t0k",
    fetch: jsonFetch({ ok: true }, calls),
    EventSource: FakeSource,
  });

  await api.send("s1", "hello");
  expect(calls).toHaveLength(1);
  expect(calls[0]!.url).toBe("/__cpd/sessions/s1/send");
  expect(calls[0]!.init?.method).toBe("POST");
  expect(new Headers(calls[0]!.init?.headers).get("x-cpd-token")).toBe("t0k");
  // The fixed one-line reason every browser send carries (web/README.md).
  expect(JSON.parse(String(calls[0]!.init?.body))).toEqual({
    text: "hello",
    unblocks: "sent from the cpd web chat",
  });
});

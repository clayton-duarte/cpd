// Dev WebSocket bridge tests (web/README.md "The WebSocket stream", docs/research/web-socket-audit.md).
// RED: `createWsBridge` still throws `not implemented`; these fail now and pass once the impl lands.
//
// The module boundary: `createWsBridge(deps)` is the injected-dependency bridge, `handle({id, token,
// send})` one connection. Every boundary is a fake — no test opens a real socket, server or
// `WebSocket`, and none sleeps. The durable host client is the `host.ts` shape (`attach(id, onFrame)`),
// the pi side an injected `readPi` + `watchPi`, and the `cpd session tail` fallback an injected `tail`.
import { expect, test, vi } from "vitest";
import { createWsBridge, type WsBridgeDeps, type WsSession } from "./ws-bridge";
import type { HostClient, HostFrame } from "./host";
import type { SnapshotFrame } from "../fold/fold";

const durableSession: WsSession = { id: "s-durable", role: "manager", runner: "durable" };
const piSession: WsSession = { id: "s-pi", role: "lead", runner: "pi" };

/** A fake durable host client: records the attached id, lets the test emit host frames, and reports the attach stop. */
function fakeHost(): {
  host: HostClient;
  emit(frame: HostFrame): void;
  attachedId(): string | undefined;
  stopped(): boolean;
} {
  let onFrame: ((frame: HostFrame) => void) | undefined;
  let id: string | undefined;
  let stopped = false;
  const host: HostClient = {
    attach(sessionId, callback) {
      id = sessionId;
      onFrame = callback;
      return () => {
        stopped = true;
      };
    },
  };
  return {
    host,
    emit: (frame) => onFrame?.(frame),
    attachedId: () => id,
    stopped: () => stopped,
  };
}

/** The deps with every boundary a harmless default; each test overrides what it exercises. */
function deps(overrides: Partial<WsBridgeDeps> = {}): WsBridgeDeps {
  return {
    token: () => "t0k",
    session: async () => undefined,
    host: fakeHost().host,
    readPi: async () => undefined,
    watchPi: () => () => {},
    tail: async () => "",
    ...overrides,
  };
}

/** Flushes pending microtasks (the handler's awaits). */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
}

/** One pi session-file line carrying a single user text. */
function piLine(text: string): string {
  return JSON.stringify({
    type: "message",
    id: "m1",
    message: { role: "user", content: [{ type: "text", text }] },
  });
}

/** A pi JSONL of `count` assistant turns, each a text of `size` characters, ids m1…mN. */
function piTurns(count: number, size: number): string {
  const lines: string[] = [];
  for (let i = 1; i <= count; i += 1) {
    lines.push(
      JSON.stringify({
        type: "message",
        id: `m${i}`,
        message: { role: "assistant", content: [{ type: "text", text: "x".repeat(size) }] },
      }),
    );
  }
  return lines.join("\n");
}

/** The messages of a snapshot frame. */
function frameMessages(frame: SnapshotFrame): Array<{ id: string; role: string; text: string }> {
  const snapshot = frame.snapshot as { messages?: Array<{ id: string; role: string; text: string }> };
  return snapshot.messages ?? [];
}

test("ws_bridge_streams_a_durable_session_from_the_host_socket", async () => {
  const fake = fakeHost();
  const bridge = createWsBridge(deps({ host: fake.host, session: async () => durableSession }));
  const sent: SnapshotFrame[] = [];
  const close = bridge.handle({ id: durableSession.id, token: "t0k", send: (frame) => sent.push(frame) });
  await flush();

  // The durable read attaches the Session's host socket.
  expect(fake.attachedId()).toBe(durableSession.id);

  // `welcome` and `attached` are handshake frames, not snapshots: nothing is sent yet.
  fake.emit({ type: "welcome", session: durableSession.id, role: "manager", epoch: "e1" });
  fake.emit({ type: "attached", req: "r1", mode: "snapshot", epoch: "e1", from_seq: 1, head_seq: 1 });
  await flush();
  expect(sent).toEqual([]);

  // The host's `snapshot` maps onto the same SnapshotFrame the SSE route sends: epoch "live", seq 1.
  fake.emit({
    type: "snapshot",
    epoch: "e1",
    seq: 1,
    snapshot: {
      type: "snapshot",
      entries: [{ model: [{ role: "user", content: [{ type: "text", text: "hello" }] }] }],
    },
  });
  await flush();

  expect(sent).toHaveLength(1);
  expect(sent[0]!.type).toBe("snapshot");
  expect(sent[0]!.epoch).toBe("live");
  expect(sent[0]!.seq).toBe(1);
  expect(frameMessages(sent[0]!)).toEqual([{ id: "host-1", role: "user", text: "hello" }]);

  // The returned closure stops the host attach.
  close();
  expect(fake.stopped()).toBe(true);
});

test("ws_bridge_streams_a_pi_session_from_its_file_and_its_watch", async () => {
  let jsonl = piLine("hello");
  let onChange: (() => void) | undefined;
  let watchStopped = false;
  const interval = vi.spyOn(globalThis, "setInterval");
  try {
    const bridge = createWsBridge(
      deps({
        session: async () => piSession,
        readPi: async () => jsonl,
        watchPi: (_id, callback) => {
          onChange = callback;
          return () => {
            watchStopped = true;
          };
        },
      }),
    );
    const sent: SnapshotFrame[] = [];
    const close = bridge.handle({ id: piSession.id, token: "t0k", send: (frame) => sent.push(frame) });
    await flush();

    // The first frame is the snapshot of the session file.
    expect(sent).toHaveLength(1);
    expect(sent[0]!.seq).toBe(1);
    expect(frameMessages(sent[0]!)).toEqual([{ id: "m1", role: "user", text: "hello" }]);

    // The watch fires: the file is re-read and a second frame is sent — no 1 s polling timer exists.
    jsonl = piLine("world");
    onChange?.();
    await flush();
    expect(sent).toHaveLength(2);
    expect(sent[1]!.seq).toBe(2);
    expect(frameMessages(sent[1]!)).toEqual([{ id: "m1", role: "user", text: "world" }]);
    expect(interval).not.toHaveBeenCalled();

    close();
    expect(watchStopped).toBe(true);
  } finally {
    interval.mockRestore();
  }
});

test("ws_bridge_carries_the_newest_turns_within_the_snapshot_budget", async () => {
  const firstFrame = async (jsonl: string): Promise<SnapshotFrame> => {
    const bridge = createWsBridge(deps({ session: async () => piSession, readPi: async () => jsonl }));
    const sent: SnapshotFrame[] = [];
    const close = bridge.handle({ id: piSession.id, token: "t0k", send: (frame) => sent.push(frame) });
    await flush();
    close();
    return sent[0]!;
  };

  // Fifty short turns: the tail bound keeps the newest 40, dropping m1…m10.
  const tail = frameMessages(await firstFrame(piTurns(50, 1)));
  expect(tail).toHaveLength(40);
  expect(tail[0]!.id).toBe("m11");
  expect(tail[tail.length - 1]!.id).toBe("m50");

  // Ten ~400 KB turns: the byte budget drops old turns until the frame fits, newest kept.
  const budgetFrame = await firstFrame(piTurns(10, 400_000));
  const budget = frameMessages(budgetFrame);
  expect(budget.length).toBeLessThan(10);
  expect(budget[budget.length - 1]!.id).toBe("m10");
  expect(JSON.stringify(budgetFrame).length).toBeLessThanOrEqual(1_000_000);
});

test("ws_bridge_refuses_a_missing_or_wrong_token_before_any_read", async () => {
  for (const token of ["", "wrong"]) {
    const fake = fakeHost();
    const session = vi.fn(async () => durableSession);
    const readPi = vi.fn(async () => undefined);
    const bridge = createWsBridge(deps({ session, readPi, host: fake.host }));
    const sent: SnapshotFrame[] = [];
    const close = bridge.handle({ id: durableSession.id, token, send: (frame) => sent.push(frame) });
    await flush();

    // Refused before any read: nothing sent, and the Session lookup, the pi read and the host attach never run.
    expect(close).toBeTypeOf("function");
    expect(sent).toEqual([]);
    expect(session).not.toHaveBeenCalled();
    expect(readPi).not.toHaveBeenCalled();
    expect(fake.attachedId()).toBeUndefined();
    close();
  }
});

test("ws_bridge_refuses_an_unknown_session", async () => {
  const fake = fakeHost();
  const readPi = vi.fn(async () => undefined);
  const tail = vi.fn(async () => "");
  const bridge = createWsBridge(deps({ session: async () => undefined, readPi, host: fake.host, tail }));
  const sent: SnapshotFrame[] = [];
  const close = bridge.handle({ id: "nope", token: "t0k", send: (frame) => sent.push(frame) });
  await flush();

  expect(close).toBeTypeOf("function");
  expect(sent).toEqual([]);
  expect(fake.attachedId()).toBeUndefined();
  expect(readPi).not.toHaveBeenCalled();
  expect(tail).not.toHaveBeenCalled();
  close();
});

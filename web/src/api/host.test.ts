// Host-socket client tests of the dev live adapter (web/README.md "`web/src/api/host.ts` — the socket
// client", hs-d819/chat-host). RED until the protocol 2 hello and frame list land.
//
// Every socket is a fake `HostConnect`: no test opens a real socket, spawns cpd or touches the
// network. The replay lines are web/src/api/host.fixture.ndjson: built from the 1.5.0 types, not recorded.
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import type { Socket } from "node:net";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createHostClient, type HostConnect, type HostFrame } from "./host";

const SESSION = "d738aa4d-ec41-4284-92db-93a88d46ac13";
const EPOCH = "157af596-ac7c-41f0-905a-faeb87c5055f";
const HOME = "/tmp/cpd-host-test-home";
const originalHome = process.env.CPD_HOME;

beforeEach(() => {
  process.env.CPD_HOME = HOME;
});

afterEach(() => {
  if (originalHome === undefined) delete process.env.CPD_HOME;
  else process.env.CPD_HOME = originalHome;
});

/** The host lines (built from the 1.5.0 types, not recorded), read from disk. */
function fixtureLines(): string[] {
  return readFileSync(new URL("./host.fixture.ndjson", import.meta.url), "utf8")
    .split("\n")
    .filter((line) => line.length > 0);
}

/** Flushes pending microtasks (the client's awaits). */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
}

/** A fake `node:net` socket: it records writes and lets the test deliver host lines. */
class FakeSocket extends EventEmitter {
  private readonly written: string[] = [];

  write(chunk: string | Uint8Array): boolean {
    this.written.push(typeof chunk === "string" ? chunk : Buffer.from(chunk).toString("utf8"));
    return true;
  }

  /** Every client line sent on this socket, parsed. */
  sent(): Array<Record<string, unknown>> {
    return this.written
      .join("")
      .split("\n")
      .filter((line) => line.length > 0)
      .map((line) => JSON.parse(line) as Record<string, unknown>);
  }

  /** The host sends one line to the client. */
  deliver(line: string | Record<string, unknown>): void {
    const text = typeof line === "string" ? line : JSON.stringify(line);
    this.emit("data", Buffer.from(`${text}\n`, "utf8"));
  }

  end(): void {
    this.emit("end");
  }

  destroy(): void {
    this.emit("close");
  }

  setEncoding(): this {
    return this;
  }

  pause(): this {
    return this;
  }

  resume(): this {
    return this;
  }
}

/** A `HostConnect` handing out fake sockets, recording the paths it was asked for. */
function fakeConnect(): { connect: HostConnect; sockets: FakeSocket[]; paths: string[] } {
  const sockets: FakeSocket[] = [];
  const paths: string[] = [];
  const connect: HostConnect = async (path) => {
    paths.push(path);
    const socket = new FakeSocket();
    sockets.push(socket);
    return socket as unknown as Socket;
  };
  return { connect, sockets, paths };
}

test("the client sends hello then attach and resolves the view", async () => {
  const { connect, sockets, paths } = fakeConnect();
  const client = createHostClient(connect);
  const frames: HostFrame[] = [];
  let resolveView!: (frame: HostFrame) => void;
  const viewFrame = new Promise<HostFrame>((resolve) => {
    resolveView = resolve;
  });

  const close = client.attach(SESSION, (frame) => {
    frames.push(frame);
    if (frame.type === "view") resolveView(frame);
  });
  await flush();

  // The path is `$CPD_HOME/run/host/<id>.sock` (web/README.md "The socket").
  expect(paths).toEqual([`${HOME}/run/host/${SESSION}.sock`]);
  const socket = sockets[0]!;

  // The host sends nothing first: the client's first line is `hello` (protocol 2).
  expect(socket.sent()[0]).toMatchObject({ type: "hello", protocol: 2, min_protocol: 2, client: "cpd web chat" });

  // After `welcome`, the client sends one `attach`, with no cursor on the first attach.
  socket.deliver(fixtureLines()[0]!);
  await flush();
  const attach = socket.sent()[1];
  expect(attach).toMatchObject({ type: "attach" });
  expect(attach?.["cursor"]).toBeUndefined();

  // The host then streams `attached`, the base `view` (seq 1) and the `ops` frame (seq 2).
  for (const line of fixtureLines().slice(1)) socket.deliver(line);
  const replayed = await viewFrame;
  expect(replayed).toMatchObject({ type: "view", epoch: EPOCH, seq: 1 });
  expect(frames.map((frame) => frame.type)).toEqual([
    "welcome",
    "attached",
    "view",
    "ops",
    "host_state",
  ]);

  close();
});

test("the client re-attaches with the last cursor after a drop", async () => {
  vi.useFakeTimers();
  try {
    const { connect, sockets, paths } = fakeConnect();
    const client = createHostClient(connect);
    const close = client.attach(SESSION, () => {});
    await flush();

    // Connection 0: the full handshake plus two stream frames, then the socket drops.
    const [welcome, attached, view, ops] = fixtureLines() as [string, string, string, string];
    sockets[0]!.deliver(welcome);
    await flush();
    sockets[0]!.deliver(attached);
    sockets[0]!.deliver(view);
    sockets[0]!.deliver(ops);
    await flush();
    sockets[0]!.emit("close");
    await vi.advanceTimersByTimeAsync(250);
    await flush();

    // The client reconnected to the same path with back-off, then hello + attach again.
    expect(paths).toEqual([`${HOME}/run/host/${SESSION}.sock`, `${HOME}/run/host/${SESSION}.sock`]);
    const second = sockets[1]!;
    expect(second.sent()[0]).toMatchObject({ type: "hello" });
    second.deliver(welcome);
    await flush();

    // The second attach carries the cursor of the last applied frame (epoch, seq 2).
    const attach = second.sent().find((line) => line["type"] === "attach");
    expect(attach?.["cursor"]).toEqual({ epoch: EPOCH, seq: 2 });

    close();
  } finally {
    vi.useRealTimers();
  }
});

test("the client refuses a frame the protocol does not allow", async () => {
  const { connect, sockets } = fakeConnect();
  const client = createHostClient(connect);
  const frames: HostFrame[] = [];
  const close = client.attach(SESSION, (frame) => frames.push(frame));
  await flush();

  const socket = sockets[0]!;
  socket.deliver(fixtureLines()[0]!);
  await flush();

  // The reader is hello/attach-only: a reply to a command it never sent (`result`) and a frame
  // outside its subset (`notice`) are refused, and a malformed line is skipped. The next allowed
  // frame still arrives.
  socket.deliver({ type: "result", req: "x", data: {} });
  socket.deliver({ type: "notice", level: "info", message: "hello" });
  socket.deliver("not json at all");
  socket.deliver(fixtureLines()[2]!);
  // The protocol 1 frames are no longer allowed.
  socket.deliver({ type: "snapshot", epoch: EPOCH, seq: 3, snapshot: {} });
  socket.deliver({ type: "events", epoch: EPOCH, seq: 4, events: [] });
  await flush();

  expect(frames.map((frame) => frame.type)).toEqual(["welcome", "view"]);

  close();
});

test("the client accepts the view and ops frames", async () => {
  const { connect, sockets } = fakeConnect();
  const client = createHostClient(connect);
  const frames: HostFrame[] = [];
  const close = client.attach(SESSION, (frame) => frames.push(frame));
  await flush();

  const socket = sockets[0]!;
  for (const line of fixtureLines()) socket.deliver(line);
  await flush();

  expect(frames.map((frame) => frame.type)).toEqual(["welcome", "attached", "view", "ops", "host_state"]);
  expect(frames[2]).toMatchObject({ type: "view", epoch: EPOCH, seq: 1 });
  expect(frames[3]).toMatchObject({ type: "ops", epoch: EPOCH, seq: 2 });

  close();
});

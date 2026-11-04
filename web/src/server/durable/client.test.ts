import { describe, expect, it } from "vitest";
import type { Disposable } from "../../protocol/runtime.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { connectHost, type HostClient, type HostFrame, type SocketFactory, type SocketLike } from "./client.ts";

class FakeSocket implements SocketLike {
  lines: string[] = [];
  ended = 0;
  private data: ((chunk: string) => void)[] = [];
  private close: ((error: Error | null) => void)[] = [];
  write(line: string): void {
    this.lines.push(line);
  }
  end(): void {
    this.ended += 1;
  }
  onData(handler: (chunk: string) => void): Disposable {
    this.data.push(handler);
    return { dispose: () => (this.data = this.data.filter((h) => h !== handler)) };
  }
  onClose(handler: (error: Error | null) => void): Disposable {
    this.close.push(handler);
    return { dispose: () => (this.close = this.close.filter((h) => h !== handler)) };
  }
  emit(chunk: string): void {
    for (const h of [...this.data]) h(chunk);
  }
  emitFrame(frame: object): void {
    this.emit(`${JSON.stringify(frame)}\n`);
  }
  emitClose(error: Error | null): void {
    for (const h of [...this.close]) h(error);
  }
  sent(): Record<string, unknown>[] {
    return this.lines.map((l) => JSON.parse(l) as Record<string, unknown>);
  }
  last(): Record<string, unknown> {
    const all = this.sent();
    const frame = all[all.length - 1];
    if (frame === undefined) throw new Error("nothing sent");
    return frame;
  }
  reply(data: Record<string, unknown>): void {
    this.emitFrame({ type: "result", req: this.last().req, data });
  }
}

const WELCOME = {
  type: "welcome",
  protocol: 1,
  host_version: "1.0.0",
  pi_durable: "1.3.0",
  session: "s1",
  role: "worker",
  epoch: "e1",
  client_id: "c1",
  cwd: "/work",
  limits: { client_line: 1000000 },
};

async function tick(): Promise<void> {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

async function open(): Promise<{ socket: FakeSocket; client: HostClient; paths: string[] }> {
  const socket = new FakeSocket();
  const paths: string[] = [];
  const factory: SocketFactory = {
    connect: async (path) => {
      paths.push(path);
      return socket;
    },
  };
  const pending = connectHost("/run/host/s1.sock", { factory, scheduler: new FakeScheduler() });
  await tick();
  socket.emitFrame(WELCOME);
  const client = await pending;
  return { socket, client, paths };
}

async function codeOf(p: Promise<unknown>): Promise<unknown> {
  try {
    await p;
  } catch (e) {
    return (e as { code?: unknown }).code;
  }
  return "resolved";
}

describe("durable client", () => {
  it("hello handshake sends protocol and client name", async () => {
    const { socket, client, paths } = await open();
    expect(paths).toEqual(["/run/host/s1.sock"]);
    expect(socket.lines[0]).toBe('{"type":"hello","protocol":1,"min_protocol":1,"client":"cpd-web"}\n');
    expect(client.welcome.pi_durable).toBe("1.3.0");
  });

  it("splits NDJSON frames across chunk boundaries", async () => {
    const { socket, client } = await open();
    const frames: HostFrame[] = [];
    client.onFrame((f) => frames.push(f));
    socket.emit('{"type":"notice","level":"info","mess');
    expect(frames).toEqual([]);
    socket.emit('age":"hi"}\n\n{"type":"status","key":"k","text":null}\n{"type":"po');
    socket.emit('ng"}\n');
    expect(frames).toEqual([
      { type: "notice", level: "info", message: "hi" },
      { type: "status", key: "k", text: null },
      { type: "pong" },
    ]);
  });

  it("attach resumes by cursor when the host allows it", async () => {
    const { socket, client } = await open();
    const p = client.attach({ epoch: "e1", seq: 5 });
    await tick();
    const sent = socket.last();
    expect(sent).toMatchObject({ type: "attach", conversation: "root", cursor: { epoch: "e1", seq: 5 } });
    expect(typeof sent.req).toBe("string");
    socket.emitFrame({ type: "attached", req: sent.req, mode: "resume", epoch: "e1", from_seq: 5, head_seq: 9, client_id: "c1" });
    expect(await p).toMatchObject({ mode: "resume", from_seq: 5, head_seq: 9 });
    const fresh = client.attach();
    await tick();
    expect(socket.last()).not.toHaveProperty("cursor");
    socket.emitFrame({ type: "attached", req: socket.last().req, mode: "snapshot", epoch: "e1", from_seq: 0, head_seq: 9, client_id: "c1" });
    expect(await fresh).toMatchObject({ mode: "snapshot" });
  });

  it("submit carries images and when_busy", async () => {
    const { socket, client } = await open();
    const p = client.submit({ text: "hi", whenBusy: "steer", requestId: "cs1", images: [{ mimeType: "image/png", dataBase64: "AAA" }] });
    await tick();
    expect(socket.last()).toMatchObject({
      type: "submit",
      text: "hi",
      when_busy: "steer",
      request_id: "cs1",
      images: [{ mime_type: "image/png", data: "AAA" }],
    });
    socket.reply({ submission_id: 3, status: "queued" });
    expect(await p).toEqual({ submissionId: 3, status: "queued" });
    const plain = client.submit({ text: "yo", whenBusy: "followUp" });
    await tick();
    expect(socket.last()).toMatchObject({ type: "submit", text: "yo", when_busy: "followUp" });
    expect(socket.last()).not.toHaveProperty("images");
    socket.reply({ submission_id: 4, status: "started" });
    await plain;
  });

  it("abort and abort_submission send their frames", async () => {
    const { socket, client } = await open();
    const a = client.abort();
    await tick();
    expect(socket.last()).toMatchObject({ type: "abort", background: false });
    socket.reply({});
    await a;
    const b = client.abortSubmission(3);
    await tick();
    expect(socket.last()).toMatchObject({ type: "abort_submission", submission_id: 3 });
    socket.reply({ outcome: "cancelled" });
    expect(await b).toEqual({ outcome: "cancelled" });
  });

  it("socket close surfaces a typed disconnect", async () => {
    const { socket, client } = await open();
    const closes: unknown[] = [];
    client.onClose((e) => closes.push((e as { code?: unknown } | null)?.code));
    const pending = client.listModels();
    await tick();
    socket.emitClose(null);
    expect(await codeOf(pending)).toBe("host_disconnected");
    expect(closes).toEqual(["host_disconnected"]);
    socket.emitClose(null);
    expect(closes).toEqual(["host_disconnected"]);
  });
});

import { describe, expect, it } from "vitest";
import { FakeScheduler } from "../testing/scheduler.ts";
import type { Disposable } from "../protocol/runtime.ts";
import {
  CoreWsClient,
  type CoreWsClientOptions,
  type CoreSnapshot,
  type FakeCoreWsSocket,
} from "./core-ws.ts";

class MockCoreWsSocket implements FakeCoreWsSocket {
  sent: string[] = [];
  private openHandlers: Array<() => void> = [];
  private msgHandlers: Array<(data: string) => void> = [];
  private closeHandlers: Array<() => void> = [];

  send(data: string): void {
    this.sent.push(data);
  }
  close(): void {
    this.emitClose();
  }
  on(event: "open" | "message" | "close", cb: any): void {
    if (event === "open") this.openHandlers.push(cb);
    else if (event === "message") this.msgHandlers.push(cb);
    else if (event === "close") this.closeHandlers.push(cb);
  }
  emitOpen(): void {
    for (const h of [...this.openHandlers]) h();
  }
  emitMessage(data: string): void {
    for (const h of [...this.msgHandlers]) h(data);
  }
  emitClose(): void {
    for (const h of [...this.closeHandlers]) h();
  }
}

describe("core-ws", () => {
  it("connects to core ws and syncs snapshot", async () => {
    let createdSocket: MockCoreWsSocket | null = null;
    let receivedSnapshot: CoreSnapshot | null = null;
    const receivedEvents: unknown[] = [];

    const client = new CoreWsClient({
      url: "ws://127.0.0.1:4567/cpd-ws",
      connectWs: (url) => {
        createdSocket = new MockCoreWsSocket();
        return createdSocket;
      },
      onSnapshot: (snap) => {
        receivedSnapshot = snap;
      },
      onEvent: (ev) => {
        receivedEvents.push(ev);
      },
    });

    client.connect();

    expect(createdSocket).not.toBeNull();
    const socket = createdSocket!;

    // Socket connects
    socket.emitOpen();

    // 1. Client must send `snapshot` request immediately on connect
    expect(socket.sent.length).toBeGreaterThanOrEqual(1);
    const snapReq = JSON.parse(socket.sent[0]!);
    expect(snapReq.method).toBe("snapshot");
    expect(snapReq.id).toBeDefined();

    // 2. Server responds with snapshot reply
    const snapPayload: CoreSnapshot = {
      contexts: [{ id: "ctx-1", title: "Architecture" }],
      plans: [{ id: "p-1", title: "Plan 1", phase: "execute" }],
      jobs: [{ plan: "p-1", key: "j-1", status: "running" }],
      cursor: 42,
    };

    socket.emitMessage(
      JSON.stringify({
        id: snapReq.id,
        result: snapPayload,
        error: null,
      }),
    );

    // Snapshot is populated on client
    expect(receivedSnapshot).toEqual(snapPayload);
    expect(client.snapshot).toEqual(snapPayload);
    expect(client.cursor).toBe(42);

    // 3. Client must subscribe to live events with returned cursor
    expect(socket.sent.length).toBeGreaterThanOrEqual(2);
    const subReq = JSON.parse(socket.sent[1]!);
    expect(subReq.method).toBe("events.subscribe");
    expect(subReq.params?.cursor).toBe(42);

    // 4. Server acknowledges subscription
    socket.emitMessage(
      JSON.stringify({
        id: subReq.id,
        result: {},
        error: null,
      }),
    );

    // 5. Server streams live event
    const liveEvent = {
      id: 43,
      plan: "p-1",
      kind: "job_status",
      data: { job: "j-1", status: "completed" },
    };
    socket.emitMessage(
      JSON.stringify({
        method: "event",
        params: { event: liveEvent },
      }),
    );

    expect(receivedEvents).toContainEqual(liveEvent);
    expect(client.cursor).toBe(43);
  });
});

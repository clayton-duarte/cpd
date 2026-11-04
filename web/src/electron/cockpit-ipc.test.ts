import type { HubConnection } from "../server/hub/connection.ts";
import { registerWebuiIpc, type IpcMainOnLike, type WebContentsLike, type WebuiIpcDeps } from "./cockpit-ipc.ts";

type Listener = Parameters<IpcMainOnLike["on"]>[1];

function fakeSender() {
  const sends: { channel: string; payload: unknown }[] = [];
  const onces: (() => void)[] = [];
  let destroyed = false;
  const sender: WebContentsLike = {
    send: (channel, payload) => {
      sends.push({ channel, payload });
    },
    isDestroyed: () => destroyed,
    once: (_event, listener) => {
      onces.push(listener);
    },
  };
  return {
    sender,
    sends,
    destroy: () => {
      destroyed = true;
      for (const l of onces) l();
    },
    markDestroyed: () => {
      destroyed = true;
    },
  };
}

function harness() {
  const listeners: Listener[] = [];
  const removed: Listener[] = [];
  const hubs: { handled: unknown[]; closes: number; send: (m: never) => void; failWith?: unknown }[] = [];
  const errors: unknown[] = [];
  let resolveStarted: () => void = () => {};
  const started = new Promise<void>((resolve) => {
    resolveStarted = resolve;
  });
  const deps: WebuiIpcDeps = {
    ipc: {
      on: (channel, listener) => {
        expect(channel).toBe("cpd:c2s");
        listeners.push(listener);
      },
      removeListener: (channel, listener) => {
        expect(channel).toBe("cpd:c2s");
        removed.push(listener);
      },
    },
    newConnection: (send) => {
      const rec: (typeof hubs)[number] = { handled: [], closes: 0, send: send as (m: never) => void };
      hubs.push(rec);
      const hub: HubConnection = {
        handle: (raw) => {
          rec.handled.push(raw);
          return rec.failWith === undefined ? Promise.resolve() : Promise.reject(rec.failWith);
        },
        close: () => {
          rec.closes++;
          return Promise.resolve();
        },
      };
      return hub;
    },
    started,
    onError: (e) => errors.push(e),
  };
  const flush = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
  };
  return { deps, listeners, removed, hubs, errors, resolveStarted: () => resolveStarted(), flush };
}

const emit = (h: ReturnType<typeof harness>, s: ReturnType<typeof fakeSender>, payload: unknown) =>
  h.listeners[0]!({ sender: s.sender }, payload);

test("registerWebuiIpc creates one hub connection per sender", () => {
  const h = harness();
  registerWebuiIpc(h.deps);
  const a = fakeSender();
  const b = fakeSender();
  emit(h, a, 1);
  emit(h, a, 2);
  expect(h.hubs).toHaveLength(1);
  emit(h, b, 3);
  expect(h.hubs).toHaveLength(2);
});

test("registerWebuiIpc forwards payloads to the hub after the cockpit started", async () => {
  const h = harness();
  registerWebuiIpc(h.deps);
  const a = fakeSender();
  emit(h, a, { n: 1 });
  emit(h, a, { n: 2 });
  await h.flush();
  expect(h.hubs[0]!.handled).toEqual([]);
  h.resolveStarted();
  await h.flush();
  expect(h.hubs[0]!.handled).toEqual([{ n: 1 }, { n: 2 }]);
});

test("registerWebuiIpc sends hub messages to the sender on cpd:s2c", () => {
  const h = harness();
  registerWebuiIpc(h.deps);
  const a = fakeSender();
  emit(h, a, 1);
  h.hubs[0]!.send({ type: "x" } as never);
  expect(a.sends).toEqual([{ channel: "cpd:s2c", payload: { type: "x" } }]);
});

test("registerWebuiIpc does not send to a destroyed sender", () => {
  const h = harness();
  registerWebuiIpc(h.deps);
  const a = fakeSender();
  emit(h, a, 1);
  a.markDestroyed();
  h.hubs[0]!.send({ type: "x" } as never);
  expect(a.sends).toEqual([]);
});

test("registerWebuiIpc closes the hub when the sender is destroyed", () => {
  const h = harness();
  registerWebuiIpc(h.deps);
  const a = fakeSender();
  emit(h, a, 1);
  a.destroy();
  expect(h.hubs[0]!.closes).toBe(1);
  emit(h, a, 2);
  expect(h.hubs).toHaveLength(2);
});

test("registerWebuiIpc dispose closes every hub and removes the listener", async () => {
  const h = harness();
  const reg = registerWebuiIpc(h.deps);
  emit(h, fakeSender(), 1);
  emit(h, fakeSender(), 2);
  const listener = h.listeners[0]!;
  await reg.dispose();
  await reg.dispose();
  expect(h.removed).toEqual([listener]);
  expect(h.hubs.map((x) => x.closes)).toEqual([1, 1]);
});

test("registerWebuiIpc reports a handle failure through onError", async () => {
  const h = harness();
  registerWebuiIpc(h.deps);
  const a = fakeSender();
  emit(h, a, 1);
  const boom = new Error("boom");
  h.hubs[0]!.failWith = boom;
  h.resolveStarted();
  await h.flush();
  expect(h.errors).toEqual([boom]);
});

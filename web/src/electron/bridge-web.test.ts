import { CHANNELS } from "./channels.ts";
import { createBridge, type IpcRendererLike } from "./bridge.ts";

type Listener = (event: unknown, ...args: unknown[]) => void;

function fakeIpc() {
  const sends: { channel: string; args: unknown[] }[] = [];
  const listeners = new Map<string, Listener[]>();
  const ipc: IpcRendererLike = {
    invoke: () => Promise.resolve(undefined),
    send(channel, ...args) {
      sends.push({ channel, args });
    },
    on(channel, listener) {
      listeners.set(channel, [...(listeners.get(channel) ?? []), listener]);
    },
    removeListener(channel, listener) {
      listeners.set(channel, (listeners.get(channel) ?? []).filter((l) => l !== listener));
    },
  };
  return { ipc, sends, listeners };
}

test("bridge sendWeb sends on cpd:c2s", () => {
  const f = fakeIpc();
  const frame = { type: "hello" };
  createBridge(f.ipc).sendWeb(frame);
  expect(f.sends).toEqual([{ channel: "cpd:c2s", args: [frame] }]);
});

test("bridge onWebMessage subscribes to cpd:s2c and unsubscribes", () => {
  const f = fakeIpc();
  const got: unknown[] = [];
  const off = createBridge(f.ipc).onWebMessage((raw) => got.push(raw));
  const registered = f.listeners.get("cpd:s2c") ?? [];
  expect(registered).toHaveLength(1);
  registered[0]!({}, { n: 1 });
  expect(got).toEqual([{ n: 1 }]);
  off();
  expect(f.listeners.get("cpd:s2c")).toEqual([]);
});

test("CHANNELS web channel names are cpd:c2s and cpd:s2c", () => {
  expect(CHANNELS.webC2s).toBe("cpd:c2s");
  expect(CHANNELS.webS2c).toBe("cpd:s2c");
  const f = fakeIpc();
  const bridge = createBridge(f.ipc);
  bridge.sendWeb(1);
  bridge.onWebMessage(() => {});
  expect(f.sends[0]?.channel).toBe(CHANNELS.webC2s);
  expect(f.listeners.has(CHANNELS.webS2c)).toBe(true);
});

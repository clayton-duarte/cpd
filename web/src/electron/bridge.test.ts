import { CHANNELS } from "./channels.ts";
import { createBridge, type IpcRendererLike } from "./bridge.ts";

type Listener = (event: unknown, ...args: unknown[]) => void;

function fakeIpc(invokeResult: unknown = undefined) {
  const invokes: { channel: string; args: unknown[] }[] = [];
  const sends: { channel: string; args: unknown[] }[] = [];
  const listeners = new Map<string, Listener[]>();
  const ipc: IpcRendererLike = {
    invoke(channel, ...args) {
      invokes.push({ channel, args });
      return Promise.resolve(invokeResult);
    },
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
  return { ipc, invokes, sends, listeners };
}

test("createBridge readWasm resolves bytes and rejects anything else", async () => {
  const bytes = new Uint8Array([1, 2, 3]);
  const ok = fakeIpc(bytes);
  await expect(createBridge(ok.ipc).readWasm()).resolves.toBe(bytes);
  expect(ok.invokes).toEqual([{ channel: CHANNELS.wasm, args: [] }]);
  for (const bad of ["abc", [1, 2], null, undefined, {}]) {
    await expect(createBridge(fakeIpc(bad).ipc).readWasm()).rejects.toThrow("invalid wasm payload");
  }
});

test("createBridge startPty invokes pty:start with the size", async () => {
  const f = fakeIpc();
  const bridge = createBridge(f.ipc);
  await expect(bridge.startPty({ cols: 80, rows: 24 })).resolves.toBeUndefined();
  expect(f.invokes).toEqual([{ channel: CHANNELS.ptyStart, args: [{ cols: 80, rows: 24 }] }]);
  const g = fakeIpc("webui");
  await expect(createBridge(g.ipc).getMode()).resolves.toBe("webui");
  expect(g.invokes).toEqual([{ channel: CHANNELS.uiGetMode, args: [] }]);
});

test("createBridge writePty and resizePty send on their channels", () => {
  const f = fakeIpc();
  const bridge = createBridge(f.ipc);
  bridge.writePty("ls\r");
  bridge.resizePty({ cols: 100, rows: 30 });
  expect(f.sends).toEqual([
    { channel: CHANNELS.ptyWrite, args: ["ls\r"] },
    { channel: CHANNELS.ptyResize, args: [{ cols: 100, rows: 30 }] },
  ]);
});

test("createBridge subscriptions pass the payload and unsubscribe", () => {
  const f = fakeIpc();
  const bridge = createBridge(f.ipc);
  const data: string[] = [];
  const exits: number[] = [];
  const modes: string[] = [];
  const offData = bridge.onPtyData((d) => data.push(d));
  const offExit = bridge.onPtyExit((c) => exits.push(c));
  const offMode = bridge.onMode((m) => modes.push(m));
  const fire = (channel: string, payload: unknown) => {
    for (const l of f.listeners.get(channel) ?? []) l({ sender: "evt" }, payload, "extra");
  };
  expect(f.listeners.get(CHANNELS.ptyData)).toHaveLength(1);
  expect(f.listeners.get(CHANNELS.ptyExit)).toHaveLength(1);
  expect(f.listeners.get(CHANNELS.uiMode)).toHaveLength(1);
  fire(CHANNELS.ptyData, "hi");
  fire(CHANNELS.ptyExit, 3);
  fire(CHANNELS.uiMode, "webui");
  expect([data, exits, modes]).toEqual([["hi"], [3], ["webui"]]);
  offData();
  offExit();
  offMode();
  expect(f.listeners.get(CHANNELS.ptyData)).toHaveLength(0);
  expect(f.listeners.get(CHANNELS.ptyExit)).toHaveLength(0);
  expect(f.listeners.get(CHANNELS.uiMode)).toHaveLength(0);
  fire(CHANNELS.ptyData, "gone");
  expect(data).toEqual(["hi"]);
});

import { CHANNELS } from "./channels.ts";
import type { PtySize } from "./channels.ts";
import { parsePtySize, parsePtyWrite, registerIpc } from "./ipc.ts";
import type { IpcDeps, IpcListener, IpcMainLike } from "./ipc.ts";
import type { PtySession, PtySinks } from "./pty.ts";

function fakeIpc() {
  const handlers = new Map<string, IpcListener>();
  const listeners = new Map<string, IpcListener>();
  const removedHandlers: string[] = [];
  const removedListeners: Array<{ channel: string; listener: IpcListener }> = [];
  const ipc: IpcMainLike = {
    handle: (c, l) => { handlers.set(c, l); },
    on: (c, l) => { listeners.set(c, l); },
    removeHandler: (c) => { removedHandlers.push(c); },
    removeListener: (c, l) => { removedListeners.push({ channel: c, listener: l }); },
  };
  return { ipc, handlers, listeners, removedHandlers, removedListeners };
}

function fakeSession() {
  const calls: string[] = [];
  const session: PtySession = {
    write: (d) => { calls.push(`write:${d}`); },
    resize: (s) => { calls.push(`resize:${s.cols}x${s.rows}`); },
    kill: () => { calls.push("kill"); },
  };
  return { session, calls };
}

function setup() {
  const f = fakeIpc();
  const sessions: Array<ReturnType<typeof fakeSession>> = [];
  const spawnedSinks: PtySinks[] = [];
  const spawnedSizes: PtySize[] = [];
  const events: string[] = [];
  const wasm = new Uint8Array([1, 2, 3]);
  const deps: IpcDeps = {
    ipc: f.ipc,
    startPty: (size, sinks) => {
      const s = fakeSession();
      sessions.push(s);
      spawnedSinks.push(sinks);
      spawnedSizes.push(size);
      return s.session;
    },
    sinks: {
      data: (c) => { events.push(`data:${c}`); },
      exit: (c) => { events.push(`exit:${c}`); },
    },
    readWasm: () => Promise.resolve(wasm),
    getMode: () => "webui",
  };
  const reg = registerIpc(deps);
  const start = (p: unknown) => f.handlers.get(CHANNELS.ptyStart)?.({}, p);
  const write = (p: unknown) => f.listeners.get(CHANNELS.ptyWrite)?.({}, p);
  const resize = (p: unknown) => f.listeners.get(CHANNELS.ptyResize)?.({}, p);
  return { ...f, reg, sessions, spawnedSinks, spawnedSizes, events, wasm, start, write, resize };
}

test("parsePtySize accepts integer cols and rows from 1 to 1000", () => {
  expect(parsePtySize({ cols: 80, rows: 24 })).toEqual({ cols: 80, rows: 24 });
  expect(parsePtySize({ cols: 1, rows: 1000 })).toEqual({ cols: 1, rows: 1000 });
  expect(Object.keys(parsePtySize({ cols: 5, rows: 6, extra: 1 }))).toEqual(["cols", "rows"]);
});

test("parsePtySize rejects invalid sizes", () => {
  const bad: unknown[] = [
    null, undefined, 5, "x", {}, { cols: 80 }, { rows: 24 },
    { cols: 0, rows: 24 }, { cols: 80, rows: 0 }, { cols: 1001, rows: 24 }, { cols: 80, rows: 1001 },
    { cols: 1.5, rows: 24 }, { cols: "80", rows: 24 }, { cols: NaN, rows: 24 }, { cols: -1, rows: 24 },
  ];
  for (const raw of bad) {
    expect(() => parsePtySize(raw)).toThrow("invalid pty size");
  }
});

test("parsePtyWrite accepts strings only", () => {
  expect(parsePtyWrite("abc")).toBe("abc");
  expect(parsePtyWrite("")).toBe("");
  for (const raw of [1, null, undefined, {}, ["a"], new Uint8Array(1)]) {
    expect(() => parsePtyWrite(raw)).toThrow("invalid pty write");
  }
});

test("registerIpc registers every channel", () => {
  const t = setup();
  expect([...t.handlers.keys()].sort()).toEqual([CHANNELS.ptyStart, CHANNELS.uiGetMode, CHANNELS.wasm].sort());
  expect([...t.listeners.keys()].sort()).toEqual([CHANNELS.ptyWrite, CHANNELS.ptyResize].sort());
});

test("ptyStart spawns once and resizes on a second start", async () => {
  const t = setup();
  expect(await t.start({ cols: 80, rows: 24 })).toBeUndefined();
  expect(t.sessions).toHaveLength(1);
  expect(t.spawnedSizes[0]).toEqual({ cols: 80, rows: 24 });
  expect(await t.start({ cols: 100, rows: 30 })).toBeUndefined();
  expect(t.sessions).toHaveLength(1);
  expect(t.sessions[0]?.calls).toEqual(["resize:100x30"]);
});

test("ptyStart spawns again after the pty exited", async () => {
  const t = setup();
  await t.start({ cols: 80, rows: 24 });
  t.spawnedSinks[0]?.data("out");
  let clearedBeforeForward = false;
  const first = t.sessions[0];
  // the next start from inside the exit forward must already spawn a new session
  const sinks = t.spawnedSinks[0];
  expect(sinks).toBeDefined();
  sinks?.exit(7);
  expect(t.events).toEqual(["data:out", "exit:7"]);
  await t.start({ cols: 90, rows: 20 });
  clearedBeforeForward = t.sessions.length === 2;
  expect(clearedBeforeForward).toBe(true);
  expect(first?.calls).toEqual([]);
  expect(t.spawnedSizes[1]).toEqual({ cols: 90, rows: 20 });
});

test("ptyWrite and ptyResize reach the session", async () => {
  const t = setup();
  await t.start({ cols: 80, rows: 24 });
  t.write("ls\r");
  t.resize({ cols: 120, rows: 40 });
  expect(t.sessions[0]?.calls).toEqual(["write:ls\r", "resize:120x40"]);
});

test("ptyWrite and ptyResize throw before the pty starts", () => {
  const t = setup();
  expect(() => t.write("x")).toThrow("pty not started");
  expect(() => t.resize({ cols: 80, rows: 24 })).toThrow("pty not started");
});

test("invalid pty payloads throw and leave the session untouched", async () => {
  const t = setup();
  await expect(Promise.resolve().then(() => t.start({ cols: 0, rows: 1 }))).rejects.toThrow("invalid pty size");
  expect(t.sessions).toHaveLength(0);
  await t.start({ cols: 80, rows: 24 });
  expect(() => t.write(5)).toThrow("invalid pty write");
  expect(() => t.resize({ cols: "a", rows: 1 })).toThrow("invalid pty size");
  expect(() => t.start({ cols: 1.5, rows: 1 })).toThrow("invalid pty size");
  expect(t.sessions).toHaveLength(1);
  expect(t.sessions[0]?.calls).toEqual([]);
});

test("uiGetMode and wasm channel answer from the deps", async () => {
  const t = setup();
  expect(await t.handlers.get(CHANNELS.uiGetMode)?.({})).toBe("webui");
  expect(await t.handlers.get(CHANNELS.wasm)?.({})).toBe(t.wasm);
});

test("dispose removes every handler and kills the session", async () => {
  const t = setup();
  await t.start({ cols: 80, rows: 24 });
  const writeL = t.listeners.get(CHANNELS.ptyWrite);
  const resizeL = t.listeners.get(CHANNELS.ptyResize);
  t.reg.dispose();
  expect([...t.removedHandlers].sort()).toEqual([CHANNELS.ptyStart, CHANNELS.uiGetMode, CHANNELS.wasm].sort());
  expect(t.removedListeners).toHaveLength(2);
  expect(t.removedListeners.find((r) => r.channel === CHANNELS.ptyWrite)?.listener).toBe(writeL);
  expect(t.removedListeners.find((r) => r.channel === CHANNELS.ptyResize)?.listener).toBe(resizeL);
  expect(t.sessions[0]?.calls).toEqual(["kill"]);
});

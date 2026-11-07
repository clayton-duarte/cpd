import { CHANNELS } from "./channels.ts";
import type { PtySize, UiMode } from "./channels.ts";
import type { UpdateTimers } from "./update/types.ts";
import type { PtySession, PtySinks } from "./pty.ts";
import type { PtyTabs } from "./pty-tabs.ts";

export type IpcListener = (event: unknown, ...args: unknown[]) => unknown;
export interface IpcMainLike {
  handle(channel: string, listener: IpcListener): void;
  on(channel: string, listener: IpcListener): void;
  removeHandler(channel: string): void;
  removeListener(channel: string, listener: IpcListener): void;
}
export interface IpcDeps {
  ipc: IpcMainLike;
  startPty(size: PtySize, sinks: PtySinks): PtySession;
  sinks: PtySinks;
  readWasm(): Promise<Uint8Array>;
  getMode(): UiMode;
  timers: UpdateTimers;
  /** The per-tab registry; when present a `tabId` on a pty payload routes there instead of the singleton. */
  tabs?: PtyTabs;
}
function isSizeValue(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 1 && value <= 1000;
}
function tabIdOf(raw: unknown): string | undefined {
  if (typeof raw === "object" && raw !== null) {
    const { tabId } = raw as { tabId?: unknown };
    if (typeof tabId === "string" && tabId !== "") return tabId;
  }
  return undefined;
}
function parsePtyTabWrite(raw: unknown): string {
  if (typeof raw === "object" && raw !== null) {
    const { data } = raw as { data?: unknown };
    if (typeof data === "string") return data;
  }
  throw new Error("invalid pty write");
}
export function parsePtySize(raw: unknown): PtySize {
  if (typeof raw === "object" && raw !== null) {
    const { cols, rows } = raw as { cols?: unknown; rows?: unknown };
    if (isSizeValue(cols) && isSizeValue(rows)) return { cols, rows };
  }
  throw new Error("invalid pty size");
}
export function parsePtyWrite(raw: unknown): string {
  if (typeof raw === "string") return raw;
  throw new Error("invalid pty write");
}
export function registerIpc(deps: IpcDeps): { dispose(): void } {
  const { ipc } = deps;
  let session: PtySession | undefined;
  const sinks: PtySinks = {
    data: (chunk) => {
      deps.sinks.data(chunk);
    },
    exit: (code) => {
      session = undefined;
      deps.sinks.exit(code);
    },
  };
  const need = (): PtySession => {
    if (!session) throw new Error("pty not started");
    return session;
  };
  const start: IpcListener = (_event, payload) => {
    const size = parsePtySize(payload);
    const tabId = tabIdOf(payload);
    if (tabId !== undefined && deps.tabs) {
      deps.tabs.start(tabId, size);
      return undefined;
    }
    if (session) {
      const live = session;
      live.resize({ cols: size.cols - 1, rows: size.rows });
      deps.timers.setTimeout(() => live.resize(size), 50);
    } else session = deps.startPty(size, sinks);
    return undefined;
  };
  const write: IpcListener = (_event, payload) => {
    const tabId = tabIdOf(payload);
    if (tabId !== undefined && deps.tabs) {
      deps.tabs.write(tabId, parsePtyTabWrite(payload));
      return;
    }
    need().write(parsePtyWrite(payload));
  };
  const resize: IpcListener = (_event, payload) => {
    const tabId = tabIdOf(payload);
    if (tabId !== undefined && deps.tabs) {
      deps.tabs.resize(tabId, parsePtySize(payload));
      return;
    }
    need().resize(parsePtySize(payload));
  };
  const getMode: IpcListener = () => deps.getMode();
  const wasm: IpcListener = () => deps.readWasm();
  ipc.handle(CHANNELS.ptyStart, start);
  ipc.handle(CHANNELS.uiGetMode, getMode);
  ipc.handle(CHANNELS.wasm, wasm);
  ipc.on(CHANNELS.ptyWrite, write);
  ipc.on(CHANNELS.ptyResize, resize);
  return {
    dispose() {
      ipc.removeHandler(CHANNELS.ptyStart);
      ipc.removeHandler(CHANNELS.uiGetMode);
      ipc.removeHandler(CHANNELS.wasm);
      ipc.removeListener(CHANNELS.ptyWrite, write);
      ipc.removeListener(CHANNELS.ptyResize, resize);
      session?.kill();
      session = undefined;
      deps.tabs?.dispose();
    },
  };
}

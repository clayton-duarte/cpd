import { CHANNELS } from "./channels.ts";
import type { PtySize, UiMode } from "./channels.ts";
import type { UpdateTimers } from "./update/types.ts";
import type { PtySession, PtySinks } from "./pty.ts";

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
}
function isSizeValue(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 1 && value <= 1000;
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
    if (session) {
      const live = session;
      live.resize({ cols: size.cols - 1, rows: size.rows });
      deps.timers.setTimeout(() => live.resize(size), 50);
    } else session = deps.startPty(size, sinks);
    return undefined;
  };
  const write: IpcListener = (_event, payload) => {
    const data = parsePtyWrite(payload);
    need().write(data);
  };
  const resize: IpcListener = (_event, payload) => {
    const size = parsePtySize(payload);
    need().resize(size);
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
    },
  };
}

import type { PtySize, UiMode } from "./channels.ts";
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
}
export function parsePtySize(_raw: unknown): PtySize {
  throw new Error("not implemented");
}
export function parsePtyWrite(_raw: unknown): string {
  throw new Error("not implemented");
}
export function registerIpc(_deps: IpcDeps): { dispose(): void } {
  throw new Error("not implemented");
}

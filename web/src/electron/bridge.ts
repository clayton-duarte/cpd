import type { CpdElectronBridge } from "./channels.ts";

export interface IpcRendererLike {
  invoke(channel: string, ...args: unknown[]): Promise<unknown>;
  send(channel: string, ...args: unknown[]): void;
  on(channel: string, listener: (event: unknown, ...args: unknown[]) => void): void;
  removeListener(channel: string, listener: (event: unknown, ...args: unknown[]) => void): void;
}

export function createBridge(_ipc: IpcRendererLike): CpdElectronBridge {
  throw new Error("not implemented");
}

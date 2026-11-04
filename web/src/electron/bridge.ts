import { CHANNELS } from "./channels.ts";
import type { CpdElectronBridge, PtySize, UiMode } from "./channels.ts";

export interface IpcRendererLike {
  invoke(channel: string, ...args: unknown[]): Promise<unknown>;
  send(channel: string, ...args: unknown[]): void;
  on(channel: string, listener: (event: unknown, ...args: unknown[]) => void): void;
  removeListener(channel: string, listener: (event: unknown, ...args: unknown[]) => void): void;
}

export function createBridge(ipc: IpcRendererLike): CpdElectronBridge {
  const subscribe = <T>(channel: string, cb: (payload: T) => void): (() => void) => {
    const listener = (_event: unknown, ...args: unknown[]): void => cb(args[0] as T);
    ipc.on(channel, listener);
    return () => ipc.removeListener(channel, listener);
  };
  return {
    async readWasm() {
      const payload = await ipc.invoke(CHANNELS.wasm);
      if (!(payload instanceof Uint8Array)) throw new Error("invalid wasm payload");
      return payload;
    },
    async getMode() {
      return (await ipc.invoke(CHANNELS.uiGetMode)) as UiMode;
    },
    async startPty(size: PtySize) {
      await ipc.invoke(CHANNELS.ptyStart, { cols: size.cols, rows: size.rows });
    },
    writePty: (data) => ipc.send(CHANNELS.ptyWrite, data),
    resizePty: (size) => ipc.send(CHANNELS.ptyResize, { cols: size.cols, rows: size.rows }),
    onPtyData: (cb) => subscribe<string>(CHANNELS.ptyData, cb),
    onPtyExit: (cb) => subscribe<number>(CHANNELS.ptyExit, cb),
    onMode: (cb) => subscribe<UiMode>(CHANNELS.uiMode, cb),
    sendWeb: (raw) => ipc.send(CHANNELS.webC2s, raw),
    onWebMessage: (cb) => subscribe<unknown>(CHANNELS.webS2c, cb),
  };
}

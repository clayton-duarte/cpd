import { CHANNELS } from "./channels.ts";
import type {
  CpdElectronBridge,
  PtyDataPayload,
  PtyExitPayload,
  PtySize,
  PtyStatusPayload,
  UiMode} from "./channels.ts";
import type { SshHost } from "./ssh-config.ts";

export interface IpcRendererLike {
  invoke(channel: string, ...args: unknown[]): Promise<unknown>;
  send(channel: string, ...args: unknown[]): void;
  on(channel: string, listener: (event: unknown, ...args: unknown[]) => void): void;
  removeListener(channel: string, listener: (event: unknown, ...args: unknown[]) => void): void;
}

export function createBridge(ipc: IpcRendererLike): Omit<CpdElectronBridge, "webSocketUrl"> {
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
    async startTabPty(tabId: string, size: PtySize) {
      await ipc.invoke(CHANNELS.ptyStart, { tabId, cols: size.cols, rows: size.rows });
    },
    writeTabPty: (tabId, data) => ipc.send(CHANNELS.ptyWrite, { tabId, data }),
    resizeTabPty: (tabId, size) =>
      ipc.send(CHANNELS.ptyResize, { tabId, cols: size.cols, rows: size.rows }),
    onTabData: (cb) => subscribe<PtyDataPayload>(CHANNELS.ptyData, cb),
    onTabExit: (cb) => subscribe<PtyExitPayload>(CHANNELS.ptyExit, cb),
    onTabStatus: (cb) => subscribe<PtyStatusPayload>(CHANNELS.ptyStatus, cb),
    listHosts: async () => (await ipc.invoke(CHANNELS.ptyHosts)) as SshHost[],
  };
}

import { contextBridge, ipcRenderer } from "electron";
import { createBridge } from "./bridge.ts";
import { UPDATE_BOOTSTRAP_CHANNEL, UPDATE_STATE_CHANNEL } from "./channels.ts";
import type { UpdateBridge } from "./channels.ts";
import type { BootstrapStatus, UpdateState } from "./update/types.ts";

const subscribe = <T>(channel: string, cb: (payload: T) => void): (() => void) => {
  const listener = (_event: unknown, payload: unknown): void => cb(payload as T);
  ipcRenderer.on(channel, listener);
  return () => {
    ipcRenderer.removeListener(channel, listener);
  };
};

const update: UpdateBridge = {
  onUpdateBootstrap: (cb: (s: BootstrapStatus) => void) => subscribe(UPDATE_BOOTSTRAP_CHANNEL, cb),
  onUpdateState: (cb: (s: UpdateState) => void) => subscribe(UPDATE_STATE_CHANNEL, cb),
};

contextBridge.exposeInMainWorld("cpdElectron", { ...createBridge(ipcRenderer), ...update });

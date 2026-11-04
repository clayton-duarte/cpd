import type { createServices } from "../server/plugin.ts";

export interface WebContentsLike {
  send(channel: string, payload: unknown): void;
  isDestroyed(): boolean;
  once(event: "destroyed", listener: () => void): unknown;
}

export interface IpcMainOnLike {
  on(channel: string, listener: (event: { sender: WebContentsLike }, payload: unknown) => void): unknown;
  removeListener(channel: string, listener: (event: { sender: WebContentsLike }, payload: unknown) => void): unknown;
}

export interface WebuiIpcDeps {
  ipc: IpcMainOnLike;
  newConnection: ReturnType<typeof createServices>["newConnection"];
  started: Promise<void>;
  onError(error: unknown): void;
}

export function registerWebuiIpc(deps: WebuiIpcDeps): { dispose(): Promise<void> } {
  void deps;
  throw new Error("not implemented");
}

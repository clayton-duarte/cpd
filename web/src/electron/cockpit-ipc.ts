import { CHANNELS } from "./channels.ts";
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
  const hubs = new Map<WebContentsLike, ReturnType<WebuiIpcDeps["newConnection"]>>();
  const listener = (event: { sender: WebContentsLike }, payload: unknown): void => {
    const sender = event.sender;
    let hub = hubs.get(sender);
    if (hub === undefined) {
      const created = deps.newConnection((m) => {
        if (!sender.isDestroyed()) sender.send(CHANNELS.webS2c, m);
      });
      hub = created;
      hubs.set(sender, created);
      sender.once("destroyed", () => {
        hubs.delete(sender);
        void created.close();
      });
    }
    const target = hub;
    void deps.started.then(() => target.handle(payload)).catch(deps.onError);
  };
  deps.ipc.on(CHANNELS.webC2s, listener);
  let disposed = false;
  return {
    async dispose() {
      if (!disposed) {
        disposed = true;
        deps.ipc.removeListener(CHANNELS.webC2s, listener);
      }
      const live = [...hubs.values()];
      hubs.clear();
      await Promise.all(live.map((h) => h.close()));
    },
  };
}

import { CHANNELS } from "./channels.ts";
import type { createServices } from "../server/plugin.ts";

export interface WebContentsLike {
  send(channel: string, payload: unknown): void;
  isDestroyed(): boolean;
  once(event: "destroyed", listener: () => void): unknown;
  removeListener?(event: "destroyed", listener: () => void): unknown;
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
  const hubs = new Map<WebContentsLike, { hub: ReturnType<WebuiIpcDeps["newConnection"]>; onDestroyed: () => void }>();
  const listener = (event: { sender: WebContentsLike }, payload: unknown): void => {
    const sender = event.sender;
    let entry = hubs.get(sender);
    if (entry === undefined) {
      const created = deps.newConnection((m) => {
        if (!sender.isDestroyed()) sender.send(CHANNELS.webS2c, m);
      });
      const onDestroyed = (): void => {
        hubs.delete(sender);
        void created.close();
      };
      entry = { hub: created, onDestroyed };
      hubs.set(sender, entry);
      sender.once("destroyed", onDestroyed);
    }
    const target = entry.hub;
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
      const live = [...hubs.entries()];
      hubs.clear();
      for (const [sender, { onDestroyed }] of live) {
        sender.removeListener?.("destroyed", onDestroyed);
      }
      await Promise.all(live.map(([, { hub }]) => hub.close()));
    },
  };
}

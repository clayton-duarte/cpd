import { WebError } from "../protocol/domain.ts";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import type { CpdElectronBridge } from "./channels.ts";

export function createElectronTransport(bridge: Pick<CpdElectronBridge, "sendWeb" | "onWebMessage">): Transport {
  let closed = false;
  let open = false;
  const messageHandlers = new Set<(raw: unknown) => void>();
  const statusHandlers = new Set<(s: "open" | "closed") => void>();
  const off = bridge.onWebMessage((raw) => {
    if (!closed) for (const h of [...messageHandlers]) h(raw);
  });
  const subscribe = <T,>(set: Set<T>, h: T): Disposable => {
    set.add(h);
    return { dispose: () => { set.delete(h); } };
  };
  return {
    connect() {
      if (closed) return;
      queueMicrotask(() => {
        if (closed) return;
        open = true;
        for (const h of [...statusHandlers]) h("open");
      });
    },
    send(raw) {
      if (closed || !open) throw new WebError("internal", "transport is not open");
      bridge.sendWeb(raw);
    },
    close() {
      if (closed) return;
      closed = true;
      off();
    },
    onMessage: (h) => subscribe(messageHandlers, h),
    onStatus: (h) => subscribe(statusHandlers, h),
  };
}

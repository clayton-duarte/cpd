import { createRoot } from "react-dom/client";
import { WebError } from "./protocol/domain.ts";
import type { Disposable, Transport } from "./protocol/runtime.ts";
import { App } from "./ui/App.tsx";
import { browserFileReader, browserScheduler } from "./ui/browser-boundaries.ts";
import "./ui/app.css";

function createHotTransport(hot: NonNullable<ImportMeta["hot"]>): Transport {
  let closed = false;
  let open = true; // the page is served over the already-connected Vite socket (O11: runtime timing unverified)
  const messageHandlers = new Set<(raw: unknown) => void>();
  const statusHandlers = new Set<(s: "open" | "closed") => void>();
  const emitStatus = (s: "open" | "closed"): void => {
    if (closed) return;
    open = s === "open";
    for (const h of [...statusHandlers]) h(s);
  };
  hot.on("cpd:s2c", (raw: unknown) => {
    if (!closed) for (const h of [...messageHandlers]) h(raw);
  });
  hot.on("vite:ws:connect", () => emitStatus("open"));
  hot.on("vite:ws:disconnect", () => emitStatus("closed"));
  const subscribe = <T,>(set: Set<T>, h: T): Disposable => {
    set.add(h);
    return { dispose: () => { set.delete(h); } };
  };
  return {
    connect() {
      if (!closed && open) queueMicrotask(() => emitStatus("open"));
    },
    send(raw) {
      if (closed || !open) throw new WebError("internal", "transport is not open");
      hot.send("cpd:c2s", raw);
    },
    close() { closed = true; },
    onMessage: (h) => subscribe(messageHandlers, h),
    onStatus: (h) => subscribe(statusHandlers, h),
  };
}

const hot = import.meta.hot;
if (hot === undefined) throw new Error("cpd web runs on the Vite dev server only (import.meta.hot is missing)");
const root = document.getElementById("root");
if (root === null) throw new Error("missing #root");
// No StrictMode: its simulated unmount would close the one-shot Connection.
createRoot(root).render(<App transport={createHotTransport(hot)} scheduler={browserScheduler} reader={browserFileReader} />);

import { registerServiceWorker } from "./state/sw-register.ts";
import { createRoot } from "react-dom/client";
import { browserFileReader, browserScheduler } from "./ui/browser-boundaries.ts";
import { browserWebSocket, createWebSocketTransport } from "./protocol/ws-transport.ts";
import { Root } from "./ui/Root.tsx";
import "./ui/app.css";

const root = document.getElementById("root");
if (root === null) throw new Error("missing #root");
// The browser build connects same-origin to the endpoint the Vite dev server hosts (15.1).
const scheme = location.protocol === "https:" ? "wss:" : "ws:";
const url = `${scheme}//${location.host}/cpd-ws`;
const transport = createWebSocketTransport({ url, createSocket: browserWebSocket, scheduler: browserScheduler });
// No StrictMode: its simulated unmount would close the one-shot Connection.
createRoot(root).render(<Root transport={transport} scheduler={browserScheduler} reader={browserFileReader} />);

// Production browser only: Electron, dev and insecure contexts skip registration (sw-register.ts).
void registerServiceWorker({
  isElectron: (window as { cpdElectron?: unknown }).cpdElectron !== undefined,
  isDev: import.meta.env.DEV,
  isSecureContext: window.isSecureContext,
  ...(navigator.serviceWorker === undefined ? {} : { serviceWorker: navigator.serviceWorker }),
}).then(async (outcome) => {
  if (outcome === "skipped") return;
  const reg = await navigator.serviceWorker.getRegistration("/");
  if (reg === undefined) return;
  const prompt = (waiting: ServiceWorker): void => {
    const button = document.createElement("button");
    button.textContent = "Reload to update";
    button.style.cssText = "position:fixed;bottom:12px;right:12px;z-index:9999;padding:8px 12px";
    button.addEventListener("click", () => {
      navigator.serviceWorker.addEventListener("controllerchange", () => location.reload(), { once: true });
      waiting.postMessage({ type: "SKIP_WAITING" });
    });
    document.body.appendChild(button);
  };
  if (reg.waiting !== null) prompt(reg.waiting);
  reg.addEventListener("updatefound", () => {
    const installing = reg.installing;
    installing?.addEventListener("statechange", () => {
      if (installing.state === "installed" && navigator.serviceWorker.controller !== null) prompt(installing);
    });
  });
});

import { createRoot } from "react-dom/client";
import { App } from "../ui/App.tsx";
import { browserFileReader, browserScheduler } from "../ui/browser-boundaries.ts";
import "../ui/app.css";
import type { CpdElectronBridge } from "./channels.ts";
import { createElectronTransport } from "./transport.ts";

export function mountWebui(
  root: HTMLElement,
  bridge: Pick<CpdElectronBridge, "sendWeb" | "onWebMessage">,
): void {
  // No StrictMode: its simulated unmount would close the one-shot Connection.
  createRoot(root).render(
    <App transport={createElectronTransport(bridge)} scheduler={browserScheduler} reader={browserFileReader} />,
  );
}

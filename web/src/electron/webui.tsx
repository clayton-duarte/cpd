import { createRoot } from "react-dom/client";
import { Root } from "../ui/Root.tsx";
import { browserFileReader, browserScheduler } from "../ui/browser-boundaries.ts";
import { browserWebSocket, createWebSocketTransport } from "../protocol/ws-transport.ts";
import "../ui/app.css";
import type { CpdElectronBridge } from "./channels.ts";
import { installCrashSignals } from "./crash-signals.ts";
import type { HealthBridge } from "./health-bridge.ts";

export function mountWebui(
  root: HTMLElement,
  bridge: Pick<CpdElectronBridge, "webSocketUrl"> & HealthBridge,
): void {
  const signals = installCrashSignals(window, bridge.reportCrash);
  // The endpoint is hosted by the main process on loopback; the URL is handed over by the preload (15.1).
  const transport = createWebSocketTransport({ url: bridge.webSocketUrl, createSocket: browserWebSocket, scheduler: browserScheduler });
  // No StrictMode: its simulated unmount would close the one-shot Connection.
  createRoot(root).render(
    <Root
      transport={transport}
      scheduler={browserScheduler}
      reader={browserFileReader}
      onCrash={bridge.reportCrash}
      onHealthy={bridge.reportHealthy}
      errors={signals.errors}
    />,
  );
}

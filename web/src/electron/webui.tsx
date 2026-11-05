import { createRoot } from "react-dom/client";
import { Root } from "../ui/Root.tsx";
import { browserFileReader, browserScheduler } from "../ui/browser-boundaries.ts";
import "../ui/app.css";
import type { CpdElectronBridge } from "./channels.ts";
import { installCrashSignals } from "./crash-signals.ts";
import type { HealthBridge } from "./health-bridge.ts";
import { createElectronTransport } from "./transport.ts";

export function mountWebui(
  root: HTMLElement,
  bridge: Pick<CpdElectronBridge, "sendWeb" | "onWebMessage"> & HealthBridge,
): void {
  const signals = installCrashSignals(window, bridge.reportCrash);
  // No StrictMode: its simulated unmount would close the one-shot Connection.
  createRoot(root).render(
    <Root
      transport={createElectronTransport(bridge)}
      scheduler={browserScheduler}
      reader={browserFileReader}
      onCrash={bridge.reportCrash}
      onHealthy={bridge.reportHealthy}
      errors={signals.errors}
    />,
  );
}

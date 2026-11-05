import type { CpdElectronBridge } from "./channels.ts";
import type { HealthBridge } from "./health-bridge.ts";

declare global {
  interface Window {
    cpdElectron: CpdElectronBridge & HealthBridge;
  }
}

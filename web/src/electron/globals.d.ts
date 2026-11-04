import type { CpdElectronBridge } from "./channels.ts";

declare global {
  interface Window {
    cpdElectron: CpdElectronBridge;
  }
}

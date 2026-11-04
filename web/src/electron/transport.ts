import type { Transport } from "../protocol/runtime.ts";
import type { CpdElectronBridge } from "./channels.ts";

export function createElectronTransport(bridge: Pick<CpdElectronBridge, "sendWeb" | "onWebMessage">): Transport {
  void bridge;
  throw new Error("not implemented");
}

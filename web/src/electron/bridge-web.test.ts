import { CHANNELS } from "./channels.ts";
import { createBridge, type IpcRendererLike } from "./bridge.ts";

const ipc: IpcRendererLike = {
  invoke: () => Promise.resolve(undefined),
  send: () => undefined,
  on: () => undefined,
  removeListener: () => undefined,
};

test("the bridge carries no WebUI IPC data channel (removed by web/CONTRACT.md 15.1)", () => {
  expect(Object.values(CHANNELS)).not.toContain("cpd:c2s");
  expect(Object.values(CHANNELS)).not.toContain("cpd:s2c");
  const bridge = createBridge(ipc) as unknown as Record<string, unknown>;
  expect("sendWeb" in bridge).toBe(false);
  expect("onWebMessage" in bridge).toBe(false);
});

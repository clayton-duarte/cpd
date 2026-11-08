// The pty-teardown regression (air-n62): a node-pty stream can emit data after its BrowserWindow
// is destroyed (the window closed, or the app is quitting), and the send path must not touch the
// dead window. Mocks only: no real Electron and no real pty. main.ts is the Electron entry, so its
// heavy collaborators are mocked and `boot` is stubbed to hang, keeping the import inert.
import { expect, test, vi } from "vitest";

vi.mock("electron", () => ({
  app: { setName: () => {}, whenReady: () => new Promise(() => {}), on: () => {}, exit: () => {} },
  BrowserWindow: class {},
  dialog: {},
  ipcMain: {},
  Menu: {},
  nativeImage: {},
  nativeTheme: {},
  powerMonitor: {},
  shell: {},
  Tray: class {},
}));
vi.mock("node-pty", () => ({ spawn: () => { throw new Error("no real pty"); } }));
vi.mock("../server/plugin.ts", () => ({ createServices: () => ({}), realBoundaries: () => ({}) }));
vi.mock("./boot.ts", () => ({ boot: () => new Promise(() => {}) }));

import { sendToWindow, type SendTarget } from "./main.ts";

/** A destroyed BrowserWindow: `isDestroyed()` is true and any read of `webContents` throws. */
function destroyedWindow(): SendTarget {
  return {
    isDestroyed: () => true,
    get webContents(): SendTarget["webContents"] {
      throw new TypeError("Object has been destroyed");
    },
  };
}

test("send does not throw after the window is destroyed", () => {
  expect(() => sendToWindow(destroyedWindow(), "pty:data", "chunk")).not.toThrow();
});

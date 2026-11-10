// The pty-teardown regression (air-n62): a node-pty stream can emit data after its BrowserWindow
// is destroyed (the window closed, or the app is quitting), and the send path must not touch the
// dead window. Mocks only: no real Electron and no real pty. main.ts is the Electron entry, so its
// heavy collaborators are mocked and `boot` is stubbed to hang, keeping the import inert.
import { expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  bootDeps: undefined as any,
  appHandlers: {} as Record<string, (event?: any) => void>,
  createServices: vi.fn((boundaries: any) => ({
    boundaries,
    cockpit: { start: vi.fn().mockResolvedValue(undefined), stop: vi.fn().mockResolvedValue(undefined) },
    newConnection: vi.fn(),
  })),
  realBoundaries: vi.fn((env: any) => ({
    cpdHome: "/mock-home/.cpd",
    scheduler: { now: () => 0, after: () => () => {} },
  })),
  writeWebuiToken: vi.fn((cpdHome: string) => ({
    token: "mock-token-abc123",
    remove: vi.fn(),
  })),
}));

vi.mock("electron", () => ({
  app: {
    setName: () => {},
    whenReady: () => Promise.resolve(),
    on: (event: string, handler: (event?: any) => void) => {
      mocks.appHandlers[event] = handler;
    },
    exit: () => {},
    quit: () => {},
    isPackaged: true,
    getAppPath: () => "/mock/app",
    getVersion: () => "0.0.0",
    setAboutPanelOptions: () => {},
    dock: { setIcon: () => {} },
  },
  BrowserWindow: class {},
  dialog: {},
  ipcMain: {},
  Menu: {
    buildFromTemplate: () => ({}),
  },
  nativeImage: {
    createEmpty: () => ({}),
    createFromPath: () => ({ setTemplateImage: () => {} }),
  },
  nativeTheme: {
    on: () => {},
  },
  powerMonitor: {},
  shell: {},
  Tray: class {
    setImage() {}
    setToolTip() {}
    setContextMenu() {}
    destroy() {}
  },
}));
vi.mock("node-pty", () => ({ spawn: () => { throw new Error("no real pty"); } }));
vi.mock("../server/plugin.ts", () => ({
  createServices: (boundaries: unknown) => mocks.createServices(boundaries),
  realBoundaries: (env: unknown) => mocks.realBoundaries(env),
}));
vi.mock("../server/webui-token.ts", () => ({
  writeWebuiToken: (cpdHome: string) => mocks.writeWebuiToken(cpdHome),
}));
vi.mock("./boot.ts", () => ({
  boot: (deps: any) => {
    mocks.bootDeps = deps;
    return new Promise(() => {});
  },
}));

Object.defineProperty(process, "resourcesPath", {
  value: "/mock/resources",
  configurable: true,
  writable: true,
});

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

test("packaged Electron boot path issues and passes webuiToken to createServices", async () => {
  // Wait for boot to be called and capture deps
  expect(mocks.bootDeps).toBeDefined();
  await mocks.bootDeps.app.whenReady();

  expect(mocks.writeWebuiToken).toHaveBeenCalledWith(expect.stringContaining(".cpd"));
  const issued = mocks.writeWebuiToken.mock.results[0]?.value;
  expect(issued).toBeDefined();

  expect(mocks.createServices).toHaveBeenCalledWith(
    expect.objectContaining({ webuiToken: issued.token }),
  );

  const beforeQuitHandler = mocks.appHandlers["before-quit"];
  expect(beforeQuitHandler).toBeDefined();
  beforeQuitHandler?.({ preventDefault: () => {} });
  expect(issued.remove).toHaveBeenCalledTimes(1);
});

import { createViewToggle, hostTabsVisible } from "./view.ts";

test("createViewToggle shows the terminal and hides the webui root", () => {
  const terminal = { hidden: true };
  const webui = { hidden: false };
  createViewToggle(terminal, webui)(true);
  expect(terminal.hidden).toBe(false);
  expect(webui.hidden).toBe(true);
});

test("createViewToggle hides the terminal and shows the webui root", () => {
  const terminal = { hidden: false };
  const webui = { hidden: true };
  createViewToggle(terminal, webui)(false);
  expect(terminal.hidden).toBe(true);
  expect(webui.hidden).toBe(false);
});

test("createViewToggle hides the tab bar when the mode is not tui", () => {
  // The host tab bar belongs to the TUI view alone (air-d50): WebUI and WebUI Preview both hide it.
  for (const mode of ["webui", "webui-preview"] as const) {
    const terminal = { hidden: false };
    const webui = { hidden: false };
    const tabBar = { hidden: false };
    createViewToggle(terminal, webui, tabBar)(false, mode);
    expect(tabBar.hidden, mode).toBe(true);
  }

  // The TUI view keeps it.
  const terminal = { hidden: true };
  const webui = { hidden: false };
  const tabBar = { hidden: true };
  createViewToggle(terminal, webui, tabBar)(true, "tui");
  expect(tabBar.hidden).toBe(false);
});

test("host tabs are shown in the tui view", () => {
  expect(hostTabsVisible("tui")).toBe(true);
});

test("host tabs are hidden in the webui view", () => {
  expect(hostTabsVisible("webui")).toBe(false);
});

test("host tabs are hidden in the webui-preview view", () => {
  // WebUI Preview runs the WebUI renderer (air-d41): the renderer receives `webui`, so the tabs stay hidden.
  expect(hostTabsVisible("webui-preview")).toBe(false);
});

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

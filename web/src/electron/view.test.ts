import { createViewToggle } from "./view.ts";

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

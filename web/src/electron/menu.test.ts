import type { UiMode } from "./channels.ts";
import { buildMenuTemplate } from "./menu.ts";

function build(mode: UiMode, platform: NodeJS.Platform, onSelect: (m: UiMode) => void = () => {}) {
  return buildMenuTemplate({ mode, platform, onSelect });
}

function viewItems(template: ReturnType<typeof build>) {
  const view = template.find((item) => item.label === "View");
  return (view?.submenu ?? []) as { label?: string; type?: string; checked?: boolean; accelerator?: string; click?: (...args: unknown[]) => void }[];
}

test("buildMenuTemplate puts appMenu first on darwin only", () => {
  const darwin = build("tui", "darwin");
  expect(darwin.map((i) => i.role ?? i.label)).toEqual(["appMenu", "editMenu", "View", "windowMenu"]);
  for (const platform of ["linux", "win32"] as const) {
    expect(build("tui", platform).map((i) => i.role ?? i.label)).toEqual(["editMenu", "View", "windowMenu"]);
  }
});

test("View menu has three radio clients with WebUI accelerated", () => {
  const items = viewItems(build("webui", "linux"));
  expect(items.map((i) => i.label)).toEqual(["TUI", "WebUI", "WebUI Preview"]);
  expect(items.map((i) => i.type)).toEqual(["radio", "radio", "radio"]);
  expect(items.map((i) => i.accelerator)).toEqual([undefined, "CmdOrCtrl+Shift+M", undefined]);
});

test("buildMenuTemplate checks the radio item of the current mode", () => {
  const checked = (mode: UiMode) => viewItems(build(mode, "linux")).map((i) => i.checked);
  expect(checked("tui")).toEqual([true, false, false]);
  expect(checked("webui")).toEqual([false, true, false]);
  expect(checked("webui-preview")).toEqual([false, false, true]);
});

test("selecting WebUI Preview calls setMode with webui-preview", () => {
  const calls: UiMode[] = [];
  const items = viewItems(build("webui", "linux", (m) => calls.push(m)));
  items[2]?.click?.({}, undefined, {});
  expect(calls).toEqual(["webui-preview"]);
});

test("buildMenuTemplate click calls onSelect with the mode", () => {
  const calls: UiMode[] = [];
  const items = viewItems(build("tui", "linux", (m) => calls.push(m)));
  items[1]?.click?.({}, undefined, {});
  items[0]?.click?.();
  expect(calls).toEqual(["webui", "tui"]);
});

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
  expect(darwin.map((i) => i.role ?? i.label)).toEqual(["appMenu", "View", "windowMenu"]);
  for (const platform of ["linux", "win32"] as const) {
    expect(build("tui", platform).map((i) => i.role ?? i.label)).toEqual(["View", "windowMenu"]);
  }
});

test("buildMenuTemplate View submenu has TUI and WebUI radio items", () => {
  const items = viewItems(build("tui", "darwin"));
  expect(items).toHaveLength(2);
  expect(items[0]).toMatchObject({ label: "TUI", type: "radio" });
  expect(items[0]?.accelerator).toBeUndefined();
  expect(items[1]).toMatchObject({ label: "WebUI", type: "radio", accelerator: "CmdOrCtrl+Shift+M" });
});

test("buildMenuTemplate checks the radio item of the current mode", () => {
  const tui = viewItems(build("tui", "linux"));
  expect([tui[0]?.checked, tui[1]?.checked]).toEqual([true, false]);
  const webui = viewItems(build("webui", "linux"));
  expect([webui[0]?.checked, webui[1]?.checked]).toEqual([false, true]);
});

test("buildMenuTemplate click calls onSelect with the mode", () => {
  const calls: UiMode[] = [];
  const items = viewItems(build("tui", "linux", (m) => calls.push(m)));
  items[1]?.click?.({}, undefined, {});
  items[0]?.click?.();
  expect(calls).toEqual(["webui", "tui"]);
});

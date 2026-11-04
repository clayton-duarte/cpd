import type { MenuItemConstructorOptions } from "electron";
import type { UiMode } from "./channels.ts";

export type MenuTemplate = MenuItemConstructorOptions[];
export interface MenuTemplateOptions {
  mode: UiMode;
  platform: NodeJS.Platform;
  onSelect(mode: UiMode): void;
}

export function buildMenuTemplate(options: MenuTemplateOptions): MenuTemplate {
  const { mode, platform, onSelect } = options;
  const template: MenuTemplate = [];
  if (platform === "darwin") template.push({ role: "appMenu" });
  template.push({
    label: "View",
    submenu: [
      { label: "TUI", type: "radio", checked: mode === "tui", click: () => onSelect("tui") },
      {
        label: "WebUI",
        type: "radio",
        checked: mode === "webui",
        accelerator: "CmdOrCtrl+Shift+M",
        click: () => onSelect("webui"),
      },
    ],
  });
  template.push({ role: "windowMenu" });
  return template;
}

import type { MenuItemConstructorOptions } from "electron";
import type { UiMode } from "./channels.ts";

export type MenuTemplate = MenuItemConstructorOptions[];
export interface MenuTemplateOptions {
  mode: UiMode;
  platform: NodeJS.Platform;
  onSelect(mode: UiMode): void;
}

export function buildMenuTemplate(_options: MenuTemplateOptions): MenuTemplate {
  throw new Error("not implemented");
}

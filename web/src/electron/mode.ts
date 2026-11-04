import type { UiMode } from "./channels.ts";
import type { MenuTemplate } from "./menu.ts";

export const DEFAULT_MODE: UiMode = "tui";

export function parseUiMode(_raw: unknown): UiMode {
  throw new Error("not implemented");
}

export function modeFilePath(_cpdHome: string): string {
  throw new Error("not implemented");
}

export interface ModeFs {
  readFile(path: string): Promise<string>;
  writeFile(path: string, data: string): Promise<void>;
  mkdir(path: string): Promise<void>;
}

export function readMode(_fs: ModeFs, _file: string): Promise<UiMode> {
  throw new Error("not implemented");
}

export function writeMode(_fs: ModeFs, _file: string, _mode: UiMode): Promise<void> {
  throw new Error("not implemented");
}

export interface ModeWindow {
  setIgnoreMenuShortcuts(ignore: boolean): void;
  sendMode(mode: UiMode): void;
}
export interface MenuHost {
  setApplicationMenu(template: MenuTemplate): void;
}
export interface ModeControllerDeps {
  fs: ModeFs;
  file: string;
  menu: MenuHost;
  window: ModeWindow;
  platform: NodeJS.Platform;
  onError(error: unknown): void;
}
export interface ModeController {
  init(): Promise<void>;
  getMode(): UiMode;
  setMode(mode: UiMode): Promise<void>;
}

export function createModeController(_deps: ModeControllerDeps): ModeController {
  throw new Error("not implemented");
}

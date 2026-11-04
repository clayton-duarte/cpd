import * as path from "node:path";
import type { UiMode } from "./channels.ts";
import { buildMenuTemplate } from "./menu.ts";
import type { MenuTemplate } from "./menu.ts";

export const DEFAULT_MODE: UiMode = "tui";

export function parseUiMode(raw: unknown): UiMode {
  if (raw === "tui" || raw === "webui") return raw;
  throw new Error("invalid ui mode: " + JSON.stringify(raw));
}

export function modeFilePath(cpdHome: string): string {
  return path.join(cpdHome, "run", "ui-mode.json");
}

export interface ModeFs {
  readFile(path: string): Promise<string>;
  writeFile(path: string, data: string): Promise<void>;
  mkdir(path: string): Promise<void>;
}

export async function readMode(fs: ModeFs, file: string): Promise<UiMode> {
  let content: string;
  try {
    content = await fs.readFile(file);
  } catch (error) {
    if (typeof error === "object" && error !== null && (error as { code?: unknown }).code === "ENOENT") {
      return DEFAULT_MODE;
    }
    throw error;
  }
  try {
    const parsed: unknown = JSON.parse(content);
    if (typeof parsed !== "object" || parsed === null) throw new Error("not an object");
    return parseUiMode((parsed as { mode?: unknown }).mode);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`invalid ui mode file ${file}: ${message}`);
  }
}

export async function writeMode(fs: ModeFs, file: string, mode: UiMode): Promise<void> {
  await fs.mkdir(path.dirname(file));
  await fs.writeFile(file, JSON.stringify({ mode }) + "\n");
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

export function createModeController(deps: ModeControllerDeps): ModeController {
  let current: UiMode = DEFAULT_MODE;
  const apply = (mode: UiMode): void => {
    deps.menu.setApplicationMenu(
      buildMenuTemplate({
        mode,
        platform: deps.platform,
        onSelect: (m) => {
          controller.setMode(m).catch(deps.onError);
        },
      }),
    );
    deps.window.setIgnoreMenuShortcuts(mode === "tui");
    deps.window.sendMode(mode);
  };
  const controller: ModeController = {
    async init() {
      current = await readMode(deps.fs, deps.file);
      apply(current);
    },
    getMode: () => current,
    async setMode(mode) {
      if (mode === current) return;
      await writeMode(deps.fs, deps.file, mode);
      current = mode;
      apply(mode);
    },
  };
  return controller;
}

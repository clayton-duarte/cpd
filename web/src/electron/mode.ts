import type { UiMode } from "./channels.ts";
import type { MenuTemplate } from "./menu.ts";

/** The view `cpd desktop` always launches in (web/ELECTRON.md, air-d40). */
export const LAUNCH_MODE: UiMode = "webui";

export function parseUiMode(raw: unknown): UiMode {
  if (raw === "tui" || raw === "webui" || raw === "webui-preview") return raw;
  throw new Error("invalid ui mode: " + JSON.stringify(raw));
}

export interface ModeFs {
  writeFile(path: string, data: string): Promise<void>;
  remove(path: string): Promise<void>;
  mkdir(path: string): Promise<void>;
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
  /** The dropped `$CPD_HOME/run/ui-mode.json`, deleted once on the first launch (air-d40). */
  legacyFile: string;
  /** The run-only `$CPD_HOME/run/ui-view.json` marker (air-n19). */
  viewFile: string;
  /** The pid written into the marker; the real adapter passes `process.pid`. */
  pid: number;
  menu: MenuHost;
  window: ModeWindow;
  platform: NodeJS.Platform;
  log(message: string): void;
  onError(error: unknown): void;
}
export interface ModeController {
  init(): Promise<void>;
  getMode(): UiMode;
  setMode(mode: UiMode): Promise<void>;
  /** Graceful quit: removes the run-only marker. */
  dispose(): Promise<void>;
}

// STUB: the launch view, the delete-once, the run-only marker and the switch are implemented by a
// later Job (web/ELECTRON.md, air-d40, air-d41, air-n19).
export function createModeController(deps: ModeControllerDeps): ModeController {
  void deps;
  let current: UiMode = LAUNCH_MODE;
  return {
    async init(): Promise<void> {},
    getMode: () => current,
    async setMode(mode: UiMode): Promise<void> {
      current = mode;
    },
    async dispose(): Promise<void> {},
  };
}

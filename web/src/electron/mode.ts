import * as path from "node:path";
import type { UiMode } from "./channels.ts";
import { buildMenuTemplate, type MenuTemplate } from "./menu.ts";
import { readPreviewMode } from "./preview-mode.ts";

/** The view `cpd desktop` always launches in (web/ELECTRON.md, air-d40). */
export const LAUNCH_MODE: UiMode = "webui";

/**
 * The mode a process booted in and the build it loaded, recorded in the run-only marker so a CI
 * failure can name them. `mode` is the process mode `boot.ts` resolves (air-d95); `build` names what
 * the process loaded: the preview dev URL or the prod build from disk.
 */
export interface BootIdentity {
  readonly mode: "production" | "preview";
  readonly build: "preview" | "prod";
}

/**
 * This process's boot identity, from the same source `boot.ts` reads: an explicit `--cpd-mode`
 * argument (which a rollback relaunch appends, air-d95) beats `CPD_MODE`. The boot mode decides the
 * build: preview loads the dev renderer, production loads the prod build from disk.
 */
export function bootIdentity(
  env: Readonly<Record<string, string | undefined>>,
  argv: readonly string[],
): BootIdentity {
  const mode = readPreviewMode(env, argv).mode;
  return { mode, build: mode === "preview" ? "preview" : "prod" };
}

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
  /**
   * The boot mode and build recorded in the marker (air-n19). Defaults to this process's own
   * identity; a caller may name it so a test does not read `process.env`/`process.argv`.
   */
  boot?: BootIdentity;
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

function isMissing(error: unknown): boolean {
  return typeof error === "object" && error !== null && (error as { code?: unknown }).code === "ENOENT";
}

/** The renderer only knows the two in-place views: preview runs the WebUI client (air-d41). */
function rendererMode(mode: UiMode): "tui" | "webui" {
  return mode === "tui" ? "tui" : "webui";
}

export function createModeController(deps: ModeControllerDeps): ModeController {
  const { fs, legacyFile, viewFile, pid } = deps;
  let current: UiMode = LAUNCH_MODE;
  // The boot identity is fixed for the process's life: `view` changes on a View menu switch, the
  // boot mode and build do not, so every marker write carries the same two boot fields.
  const boot = deps.boot ?? bootIdentity(process.env, process.argv.slice(1));

  const writeMarker = async (view: UiMode): Promise<void> => {
    await fs.mkdir(path.dirname(viewFile));
    await fs.writeFile(viewFile, JSON.stringify({ view, pid, mode: boot.mode, build: boot.build }) + "\n");
  };

  const apply = async (mode: UiMode): Promise<void> => {
    deps.menu.setApplicationMenu(
      buildMenuTemplate({
        mode,
        platform: deps.platform,
        onSelect: (next) => {
          controller.setMode(next).catch(deps.onError);
        },
      }),
    );
    deps.window.setIgnoreMenuShortcuts(mode === "tui");
    deps.window.sendMode(rendererMode(mode));
  };

  // The dropped ui-mode.json is deleted once; it is never read as a launch default (air-d40).
  const deleteLegacy = async (): Promise<void> => {
    try {
      await fs.remove(legacyFile);
      deps.log(`deleted ${legacyFile}; the launch view is always ${LAUNCH_MODE}`);
    } catch (error) {
      if (!isMissing(error)) deps.onError(error);
    }
  };

  const controller: ModeController = {
    async init(): Promise<void> {
      await deleteLegacy();
      await writeMarker(current);
      await apply(current);
    },
    getMode: () => current,
    async setMode(mode: UiMode): Promise<void> {
      if (mode === current) return;
      current = mode;
      await writeMarker(mode);
      await apply(mode);
    },
    async dispose(): Promise<void> {
      try {
        await fs.remove(viewFile);
      } catch (error) {
        if (!isMissing(error)) deps.onError(error);
      }
    },
  };
  return controller;
}

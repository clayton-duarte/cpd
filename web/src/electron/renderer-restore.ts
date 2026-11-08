/**
 * The production-renderer restore seam (CONTRACT.md §19.3): leaving the WebUI Preview — the User's
 * next View menu change, a watchdog rollback included — reloads the built production renderer into
 * the window, so a dead preview page never blocks the View change. `main.ts` and the watchdog
 * rollback share this one path.
 */

/** The built production renderer, relative to `web/dist-electron` (the file `boot.ts` loads). */
export const PRODUCTION_RENDERER = "renderer/electron.html";

export interface RendererRestorePorts {
  /** Loads a built renderer file into the window: `BrowserWindow.loadFile`, bound to the window. */
  loadFile(file: string): Promise<unknown>;
}

/**
 * Restores the production renderer after leaving the preview: the window reloads the built
 * renderer, so a dead preview page never blocks the View change (§19.3).
 */
export async function restoreRenderer(ports: RendererRestorePorts): Promise<void> {
  await ports.loadFile(PRODUCTION_RENDERER);
}

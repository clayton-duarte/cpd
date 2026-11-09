import { readPreviewMode } from './preview-mode';
export interface BootWindow {
  /**
   * Whether the window itself is destroyed (a real BrowserWindow's `isDestroyed`). Optional so a
   * test double that only exposes `webContents` still satisfies this interface; a real window has it.
   */
  isDestroyed?(): boolean;
  webContents: { isDestroyed(): boolean; reloadIgnoringCache(): void };
}
export interface BootDeps {
  app: { setName(name: string): void; whenReady(): Promise<unknown> };
  BrowserWindow: new (options: { title: string; [key: string]: unknown }) => BootWindow;
  powerMonitor: unknown;
  process: { on(signal: string, handler: () => void): unknown };
  startAutoUpdate(win: BootWindow): Promise<{ bootstrap: { state: string } }>;
  /** §19.1: promotes a ready `web/dist-electron.next` before the window opens. */
  promoteOnLaunch?(): Promise<void>;
  startServices(): void;
  initMode(win: BootWindow): Promise<void>;
  registerIpc(): void;
  registerWebuiIpc(): void;
  loadDevUrl(win: BootWindow, url: string): Promise<unknown>;
  loadFile(win: BootWindow, path: string): Promise<unknown>;
  env: Record<string, string | undefined>;
  reloadServer(): Promise<void>;
}
export async function boot(deps: BootDeps): Promise<void> {
  const preview = readPreviewMode(deps.env);
  // Test-only hidden seam: the e2e suite sets CPD_ELECTRON_HIDDEN=1 so the window is
  // created with `show:false` and can never appear on the User's screen (no flash).
  // Unset in production, where Electron's default `show:true` still applies.
  const hidden = deps.env['CPD_ELECTRON_HIDDEN'] === '1';
  // Test-only step trace (CPD_BOOT_TRACE=1, set by the e2e suite): each awaited boot step prints
  // one `[boot] <step>` line to stdout, so a boot that never reaches loadDevUrl names the step it
  // stopped at in the CI log instead of a bare firstWindow timeout. Unset in production.
  const trace = deps.env['CPD_BOOT_TRACE'] === '1';
  let step = 'start';
  const mark = (next: string): void => {
    step = next;
    if (trace) console.log(`[boot] ${next}`);
  };
  try {
    deps.app.setName('CPD');
    mark('whenReady-start');
    await deps.app.whenReady();
    mark('whenReady-done');
    mark('promote-start');
    await deps.promoteOnLaunch?.();
    mark('promote-done');
    const win = new deps.BrowserWindow({ title: 'CPD', show: !hidden });
    mark('window-created');
    await deps.initMode(win);
    mark('initMode-done');
    deps.process.on('SIGUSR1', async () => {
      try {
        await deps.reloadServer();
      } catch (error) {
        console.error(error);
      }
      // air-n62: on a destroyed window, reading `win.webContents` throws `Object has been
      // destroyed` before `webContents.isDestroyed()` can run, so ask the window itself first.
      if (win.isDestroyed?.()) return;
      if (!win.webContents.isDestroyed()) win.webContents.reloadIgnoringCache();
    });
    deps.registerIpc();
    deps.registerWebuiIpc();
    mark('ipc-registered');
    const update = await deps.startAutoUpdate(win);
    mark('startUpdate-done');
    if (update.bootstrap.state === 'ready') deps.startServices();
    mark('services-started');
    const devUrl = preview.mode === 'preview' ? preview.devUrl : null;
    if (devUrl) {
      mark('loadDevUrl-start');
      try {
        await deps.loadDevUrl(win, devUrl);
      } catch (error) {
        console.error(`dev URL ${devUrl} failed to load; loading the built renderer`, error);
        await deps.loadFile(win, 'renderer/electron.html');
      }
    } else {
      mark('loadFile-start');
      await deps.loadFile(win, 'renderer/electron.html');
    }
    mark('loadDevUrl-done');
  } catch (error) {
    console.error(`[boot] failed at ${step}`, error);
    throw error;
  }
}

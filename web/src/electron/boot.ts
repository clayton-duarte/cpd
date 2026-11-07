import { readPreviewMode } from './preview-mode';
export interface BootWindow {
  webContents: { isDestroyed(): boolean; reloadIgnoringCache(): void };
}
export interface BootDeps {
  app: { setName(name: string): void; whenReady(): Promise<unknown> };
  BrowserWindow: new (options: { title: string; [key: string]: unknown }) => BootWindow;
  powerMonitor: unknown;
  process: { on(signal: string, handler: () => void): unknown };
  startAutoUpdate(win: BootWindow): Promise<{ bootstrap: { state: string } }>;
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
  deps.app.setName('CPD');
  await deps.app.whenReady();
  const win = new deps.BrowserWindow({ title: 'CPD', show: !hidden });
  await deps.initMode(win);
  deps.process.on('SIGUSR1', async () => {
    try {
      await deps.reloadServer();
    } catch (error) {
      console.error(error);
    }
    if (!win.webContents.isDestroyed()) win.webContents.reloadIgnoringCache();
  });
  deps.registerIpc();
  deps.registerWebuiIpc();
  const update = await deps.startAutoUpdate(win);
  if (update.bootstrap.state === 'ready') deps.startServices();
  const devUrl = preview.mode === 'preview' ? preview.devUrl : null;
  if (devUrl) {
    try {
      await deps.loadDevUrl(win, devUrl);
    } catch (error) {
      console.error(`dev URL ${devUrl} failed to load; loading the built renderer`, error);
      await deps.loadFile(win, 'renderer/electron.html');
    }
  } else await deps.loadFile(win, 'renderer/electron.html');
}

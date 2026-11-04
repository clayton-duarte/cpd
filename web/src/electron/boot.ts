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
  registerIpc(): void;
  registerWebuiIpc(): void;
  loadDevUrl(win: BootWindow, url: string): Promise<unknown>;
  loadFile(win: BootWindow, path: string): Promise<unknown>;
  env: Record<string, string | undefined>;
}
export async function boot(deps: BootDeps): Promise<void> {
  deps.app.setName('CPD');
  await deps.app.whenReady();
  const win = new deps.BrowserWindow({ title: 'CPD' });
  deps.process.on('SIGUSR1', () => {
    if (!win.webContents.isDestroyed()) win.webContents.reloadIgnoringCache();
  });
  deps.registerIpc();
  deps.registerWebuiIpc();
  const update = await deps.startAutoUpdate(win);
  if (update.bootstrap.state === 'ready') deps.startServices();
  const devUrl = deps.env['CPD_ELECTRON_DEV_URL'];
  if (devUrl) await deps.loadDevUrl(win, devUrl);
  else await deps.loadFile(win, 'renderer/electron.html');
}

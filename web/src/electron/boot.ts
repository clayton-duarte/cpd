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
export async function boot(_deps: BootDeps): Promise<void> {
  throw new Error('not implemented');
}

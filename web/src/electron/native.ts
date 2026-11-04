import type { AttentionCounts } from "./attention.ts";
import { join } from "node:path";
import type { UiMode } from "./channels.ts";
import { updateMenuItem } from "./update/menu.ts";
import type { UpdateState } from "./update/types.ts";

export interface AssetPaths { icon: string; tray: string; trayAttention: string }

export function resolveAssetPaths(o: { packaged: boolean; resourcesPath: string; appPath: string }): AssetPaths {
  const base = o.packaged ? o.resourcesPath : join(o.appPath, "build");
  return {
    icon: join(base, "icon.png"),
    tray: join(base, "tray", "trayTemplate.png"),
    trayAttention: join(base, "tray", "trayAttentionTemplate.png"),
  };
}

export interface ImageLike { setTemplateImage(template: boolean): void }
export interface TrayLike { setImage(image: ImageLike): void; setToolTip(text: string): void; setContextMenu(menu: unknown): void; destroy(): void }
export interface NativeShellDeps {
  platform: NodeJS.Platform;
  app: {
    dock?: { setIcon(path: string): void; setBadge(text: string): void; bounce(type: "informational" | "critical"): number };
    setAboutPanelOptions(o: { applicationName: string; applicationVersion: string; credits: string }): void;
    getVersion(): string;
  };
  nativeTheme: { themeSource: "system" | "light" | "dark" };
  nativeImage: { createFromPath(path: string): ImageLike };
  createTray(image: ImageLike): TrayLike;
  buildMenu(template: { label?: string; type?: "separator"; enabled?: boolean; click?: () => void }[]): unknown;
  isWindowFocused(): boolean;
  assets: AssetPaths;
  actions: { openCockpit(): void; setMode(mode: UiMode): void; quit(): void };
}
export interface NativeShell { setAttention(c: AttentionCounts): void; setUpdateState(state: UpdateState, onRestart: () => void): void; dispose(): void }

export function setupNativeShell(deps: NativeShellDeps): NativeShell {
  const { app, actions } = deps;
  deps.nativeTheme.themeSource = "dark";
  app.setAboutPanelOptions({ applicationName: "cpd", applicationVersion: app.getVersion(), credits: "Continuous Parallel Delivery" });
  if (deps.platform !== "darwin") return { setAttention() {}, setUpdateState() {}, dispose() {} };

  app.dock?.setIcon(deps.assets.icon);
  const normal = deps.nativeImage.createFromPath(deps.assets.tray);
  normal.setTemplateImage(true);
  const attention = deps.nativeImage.createFromPath(deps.assets.trayAttention);
  attention.setTemplateImage(true);
  const tray = deps.createTray(normal);
  tray.setToolTip("cpd");
  const buildTrayMenu = (update: { label: string; enabled: boolean; click?: () => void } | null): void => {
    tray.setContextMenu(deps.buildMenu([
      { label: "Open cockpit", click: () => actions.openCockpit() },
      { label: "Switch to TUI", click: () => actions.setMode("tui") },
      { label: "Switch to WebUI", click: () => actions.setMode("webui") },
      ...(update ? [update] : []),
      { type: "separator" },
      { label: "Quit cpd", click: () => actions.quit() },
    ]));
  };
  buildTrayMenu(null);

  let previousBlocking: number | null = null;
  let destroyed = false;
  return {
    setAttention(c) {
      const needsYou = c.blocking + c.decisions;
      app.dock?.setBadge(needsYou > 0 ? String(needsYou) : "");
      tray.setImage(needsYou > 0 ? attention : normal);
      if (previousBlocking !== null && c.blocking > previousBlocking && !deps.isWindowFocused()) app.dock?.bounce("informational");
      previousBlocking = c.blocking;
    },
    setUpdateState(state, onRestart) {
      if (!destroyed) buildTrayMenu(updateMenuItem(state, onRestart));
    },
    dispose() {
      if (destroyed) return;
      destroyed = true;
      tray.destroy();
    },
  };
}

export function createExternalLinkHandler(openExternal: (url: string) => unknown): (details: { url: string }) => { action: "deny" } {
  return (details) => {
    openExternal(details.url);
    return { action: "deny" };
  };
}

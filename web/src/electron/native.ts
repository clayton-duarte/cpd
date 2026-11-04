import type { AttentionCounts } from "./attention.ts";
import type { UiMode } from "./channels.ts";

export interface AssetPaths { icon: string; tray: string; trayAttention: string }

export function resolveAssetPaths(_o: { packaged: boolean; resourcesPath: string; appPath: string }): AssetPaths {
  throw new Error("not implemented");
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
  buildMenu(template: { label?: string; type?: "separator"; click?: () => void }[]): unknown;
  isWindowFocused(): boolean;
  assets: AssetPaths;
  actions: { openCockpit(): void; setMode(mode: UiMode): void; quit(): void };
}
export interface NativeShell { setAttention(c: AttentionCounts): void; dispose(): void }

export function setupNativeShell(_deps: NativeShellDeps): NativeShell {
  throw new Error("not implemented");
}

export function createExternalLinkHandler(_openExternal: (url: string) => unknown): (details: { url: string }) => { action: "deny" } {
  throw new Error("not implemented");
}

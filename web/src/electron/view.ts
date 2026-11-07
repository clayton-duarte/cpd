import type { UiMode } from "./channels.ts";

/**
 * Toggles the terminal and WebUI containers, and — when the tab bar is given — the host tab bar,
 * which only the `tui` mode shows (air-d50). `mode` is the app-level view mode.
 *
 * The tab bar is a stub: it is accepted but never touched yet.
 */
export function createViewToggle(
  terminal: { hidden: boolean },
  webui: { hidden: boolean },
  tabBar: { hidden: boolean } | null = null,
): (terminalVisible: boolean, mode?: UiMode) => void {
  return (terminalVisible) => {
    terminal.hidden = !terminalVisible;
    webui.hidden = terminalVisible;
    void tabBar;
  };
}

/**
 * The host tab bar is visible only in the TUI view: it is hidden in WebUI and WebUI Preview (air-d50).
 * The mode is the app-level view mode; `webui-preview` runs the WebUI renderer, so only `tui` shows the tabs.
 */
export function hostTabsVisible(mode: UiMode): boolean {
  return mode === "tui";
}

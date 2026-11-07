import type { UiMode } from "./channels.ts";

export function createViewToggle(
  terminal: { hidden: boolean },
  webui: { hidden: boolean },
): (terminalVisible: boolean) => void {
  return (terminalVisible) => {
    terminal.hidden = !terminalVisible;
    webui.hidden = terminalVisible;
  };
}

/**
 * The host tab bar is visible only in the TUI view: it is hidden in WebUI and WebUI Preview (air-d50).
 * The mode is the app-level view mode; `webui-preview` runs the WebUI renderer, so only `tui` shows the tabs.
 */
export function hostTabsVisible(_mode: UiMode): boolean {
  throw new Error("hostTabsVisible is not implemented");
}

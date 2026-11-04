export function createViewToggle(
  terminal: { hidden: boolean },
  webui: { hidden: boolean },
): (terminalVisible: boolean) => void {
  return (terminalVisible) => {
    terminal.hidden = !terminalVisible;
    webui.hidden = terminalVisible;
  };
}

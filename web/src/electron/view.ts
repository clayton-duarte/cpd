export function createViewToggle(
  terminal: { hidden: boolean },
  webui: { hidden: boolean },
): (terminalVisible: boolean) => void {
  void terminal;
  void webui;
  throw new Error("not implemented");
}

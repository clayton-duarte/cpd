/** The imperative side of a resizable panel (`usePanelRef().current`). */
export interface PanelApi {
  resize(size: string): void;
}

export interface LayoutPanels {
  readonly tree: PanelApi;
  readonly center: PanelApi;
  readonly inbox: PanelApi;
  readonly plan: PanelApi;
}

/** Reset the panel sizes to the TUI defaults: tree 20, center 58, inbox 22, plan 40. */
export function resetLayout(panels: LayoutPanels): void {
  panels.tree.resize("20%");
  panels.center.resize("58%");
  panels.inbox.resize("22%");
  panels.plan.resize("40%");
}

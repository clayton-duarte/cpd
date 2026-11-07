/**
 * The tab bar model: the local tab plus one remote tab per ssh host, and the per-tab status label.
 * Contract: web/ELECTRON.md, section `## Remote TUI tabs over ssh`.
 */
import type { UiMode } from "./channels.ts";

/** The id of the one local tab. */
export const LOCAL_TAB_ID = "local";

export type TabKind = "local" | "remote";

export type TabStatus = "connecting" | "live" | "exited" | "failed";

/** A host a remote tab is built for: the alias is both the tab label and the ssh id suffix. */
export interface TabHost {
  alias: string;
}

export interface Tab {
  id: string;
  label: string;
  kind: TabKind;
  status: TabStatus;
  /** The exit code, present on an `exited` or `failed` tab when the process reported one. */
  code?: number;
}

/** The tab id of a remote tab: `ssh:<alias>`. */
export function remoteTabId(alias: string): string {
  return "ssh:" + alias;
}

/** The local tab first, then one remote tab per host, in order; every tab starts `connecting`. */
export function buildTabs(hosts: readonly TabHost[]): Tab[] {
  return [
    { id: LOCAL_TAB_ID, label: "local", kind: "local", status: "connecting" },
    ...hosts.map((host): Tab => ({ id: remoteTabId(host.alias), label: host.alias, kind: "remote", status: "connecting" })),
  ];
}

/** The tabs visible in a view mode. Remote tabs stay visible in every mode; the mode only decides the local tab's view. */
export function tabsForMode(tabs: readonly Tab[], _mode: UiMode): Tab[] {
  return [...tabs];
}

/** The status text shown next to a tab. */
export function statusLabel(tab: Tab): string {
  switch (tab.status) {
    case "connecting":
      return "connecting...";
    case "live":
      return "";
    case "exited":
      return tab.code === undefined ? "exited" : `exited (code ${tab.code})`;
    case "failed":
      if (tab.code === 127 && tab.kind === "remote") return `cpd not found on ${tab.label}`;
      if (tab.code !== undefined) return `ssh failed (exit ${tab.code})`;
      return "ssh failed";
  }
}

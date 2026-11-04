import type { InboxRow, Urgency } from "../../../protocol/cockpit.ts";
import { type InboxTier, inboxDisplay } from "../../dango-status.ts";

export const urgencies: readonly Urgency[] = ["red", "yellow", "blue", "magenta", "cyan", "white"];

export const tierText: Readonly<Record<InboxTier, string>> = {
  1: "text-status-blocked",
  2: "text-status-landing",
  3: "text-status-focus",
  4: "text-status-decision",
  5: "text-status-active",
  6: "text-muted-foreground",
};

export function age(s: number): string {
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

/** Rows in display order: tier order, then source order. */
export function displayOrder(rows: readonly InboxRow[]): InboxRow[] {
  return urgencies.flatMap((u) => rows.filter((r) => inboxDisplay(r.urgency).tier === inboxDisplay(u).tier));
}

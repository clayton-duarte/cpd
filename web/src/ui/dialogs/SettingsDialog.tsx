import type { ReactElement } from "react";

export const SWITCHES = ["durable_workers", "write_curation", "failure_triage", "turn_question_guard"] as const;
export type SwitchName = (typeof SWITCHES)[number];

/** The read of `cpd config` / `cpd config experimental` the dialog shows. */
export interface SettingsRead {
  readonly tiers: { readonly auxiliar: string; readonly worker: string; readonly lead: string; readonly manager: string };
  readonly experimental: Readonly<Record<SwitchName, boolean>>;
}

/** Only the values the User changed. */
export interface SettingsChanges {
  readonly tiers?: Partial<SettingsRead["tiers"]>;
  readonly experimental?: Partial<Record<SwitchName, boolean>>;
}

export function SettingsDialog(_props: {
  open: boolean;
  read: SettingsRead;
  onSave(changes: SettingsChanges): void;
  onCancel(): void;
}): ReactElement | null {
  return null;
}

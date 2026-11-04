import type { ReactElement } from "react";

export interface LogEvent {
  readonly id: number;
  /** RFC 3339 timestamp. */
  readonly at: string;
  readonly kind: string;
  readonly text: string;
}

export function LogsDialog(_props: { open: boolean; onClose(): void; events: readonly LogEvent[] }): ReactElement | null {
  return null;
}

import React from "react";
import type { OutboxEntry } from "../state/outbox.ts";

export interface OutboxListProps {
  entries: readonly OutboxEntry[];
  onRetry: (id: string) => void;
  onDiscard: (id: string) => void;
}

/** Unsent work: each entry with its state; a failed one shows its error with Retry and Discard. */
export function OutboxList({ entries, onRetry, onDiscard }: OutboxListProps): React.ReactElement | null {
  if (entries.length === 0) return null;
  return (
    <ul data-part="outbox-list" aria-label="Outbox" className="border-b px-3 py-1 text-sm">
      {entries.map((e) => (
        <li key={e.id} data-part="outbox-entry" data-state={e.state} className="flex items-center gap-2">
          <span>{e.kind}</span>
          <span data-part="outbox-state">{e.state}</span>
          {e.state === "failed" && (
            <>
              <span data-part="outbox-error" className="text-destructive">{e.error}</span>
              <button type="button" onClick={() => onRetry(e.id)}>Retry</button>
              <button type="button" onClick={() => onDiscard(e.id)}>Discard</button>
            </>
          )}
        </li>
      ))}
    </ul>
  );
}

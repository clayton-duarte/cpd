import type { InboxModel } from "../protocol/cockpit.ts";
import type { Disposable } from "../protocol/runtime.ts";
import type { CockpitService } from "../server/cockpit/events.ts";

export interface AttentionCounts { blocking: number; decisions: number }

export function countAttention(inbox: InboxModel | null): AttentionCounts {
  let blocking = 0;
  let decisions = 0;
  for (const row of inbox?.rows ?? []) {
    if (row.urgency === "red") blocking++;
    else if (row.urgency === "magenta") decisions++;
  }
  return { blocking, decisions };
}

export function watchAttention(cockpit: Pick<CockpitService, "snapshot" | "subscribe">, onChange: (c: AttentionCounts) => void): Disposable {
  let active = true;
  let last = countAttention(cockpit.snapshot().inbox);
  onChange(last);
  const sub = cockpit.subscribe(() => {
    if (!active) return;
    const next = countAttention(cockpit.snapshot().inbox);
    if (next.blocking === last.blocking && next.decisions === last.decisions) return;
    last = next;
    onChange(next);
  });
  return {
    dispose() {
      if (!active) return;
      active = false;
      sub.dispose();
    },
  };
}

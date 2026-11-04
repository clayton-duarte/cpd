import type { InboxModel } from "../protocol/cockpit.ts";
import type { Disposable } from "../protocol/runtime.ts";
import type { CockpitService } from "../server/cockpit/events.ts";

export interface AttentionCounts { blocking: number; decisions: number }

export function countAttention(_inbox: InboxModel | null): AttentionCounts {
  throw new Error("not implemented");
}

export function watchAttention(_cockpit: Pick<CockpitService, "snapshot" | "subscribe">, _onChange: (c: AttentionCounts) => void): Disposable {
  throw new Error("not implemented");
}

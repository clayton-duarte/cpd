import { createDedupStore, type DedupStorage, type DedupStore } from "./hub/dedup.ts";

/**
 * Non-owning receipt seam, not a production activation mechanism. The caller
 * must fence admission, verify ownership and unsettled work, and drain and
 * dispose the old writer before resolving the callback. Production activation
 * and shared engine/adapter/schema/migration reconciliation remain disabled.
 */
export function prepareHandoff(storage: DedupStorage): {
  transfer(drainAndDispose: () => Promise<void>): Promise<DedupStore>;
} {
  let attempted = false;
  return {
    async transfer(drainAndDispose) {
      if (attempted) throw new Error("handoff already attempted");
      attempted = true;
      await drainAndDispose();
      return createDedupStore({ storage, cap: 1000 });
    },
  };
}

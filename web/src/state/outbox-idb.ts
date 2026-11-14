import { openDB } from "idb";
import type { OutboxEntry, OutboxStorage } from "./outbox.ts";

const STORE = "entries";

/** The outbox's IndexedDB storage. Asks the browser for persistent storage once, when first used. */
export function createIdbOutboxStorage(dbName = "cpd-outbox"): OutboxStorage {
  const db = openDB(dbName, 1, {
    upgrade(d) {
      d.createObjectStore(STORE, { keyPath: "id" });
    },
  });
  let persistAsked = false;
  const askPersist = (): void => {
    if (persistAsked) return;
    persistAsked = true;
    void globalThis.navigator?.storage?.persist?.();
  };
  return {
    async load() {
      askPersist();
      return (await db).getAll(STORE) as Promise<OutboxEntry[]>;
    },
    async put(entry) {
      askPersist();
      await (await db).put(STORE, entry);
    },
    async remove(id) {
      await (await db).delete(STORE, id);
    },
  };
}

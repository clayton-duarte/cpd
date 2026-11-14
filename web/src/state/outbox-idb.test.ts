import "fake-indexeddb/auto";
import { afterEach, expect, it, vi } from "vitest";
import type { OutboxEntry } from "./outbox.ts";
import { createIdbOutboxStorage } from "./outbox-idb.ts";

const entry = (id: string): OutboxEntry => ({
  id, kind: "action", payload: { id }, state: "queued", createdAt: "2026-01-01T00:00:00Z",
});
let n = 0;
const name = (): string => `outbox-test-${n++}`;
afterEach(() => vi.unstubAllGlobals());

it("put then load returns the entry", async () => {
  const s = createIdbOutboxStorage(name());
  await s.put(entry("a"));
  expect(await s.load()).toEqual([entry("a")]);
});
it("put replaces an entry with the same id", async () => {
  const s = createIdbOutboxStorage(name());
  await s.put(entry("a"));
  await s.put({ ...entry("a"), state: "failed", error: "boom" });
  expect(await s.load()).toEqual([{ ...entry("a"), state: "failed", error: "boom" }]);
});
it("remove deletes the entry", async () => {
  const s = createIdbOutboxStorage(name());
  await s.put(entry("a"));
  await s.remove("a");
  expect(await s.load()).toEqual([]);
});
it("a second storage on the same database sees the entries", async () => {
  const db = name();
  await createIdbOutboxStorage(db).put(entry("a"));
  expect(await createIdbOutboxStorage(db).load()).toEqual([entry("a")]);
});
it("asks the browser for persistent storage once, on first use", async () => {
  const persist = vi.fn(async () => true);
  vi.stubGlobal("navigator", { storage: { persist } });
  const s = createIdbOutboxStorage(name());
  expect(persist).not.toHaveBeenCalled();
  await s.put(entry("a"));
  await s.put(entry("b"));
  expect(persist).toHaveBeenCalledTimes(1);
});

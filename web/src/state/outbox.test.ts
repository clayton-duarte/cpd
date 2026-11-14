import { expect, it } from "vitest";
import { createOutbox, type OutboxEntry, type SendResult } from "./outbox";

function setup(
  initial: OutboxEntry[] = [],
  result: () => SendResult = () => ({ ok: true }),
) {
  const store = new Map<string, OutboxEntry>(initial.map((e) => [e.id, e]));
  const sent: string[] = [];
  const storage = {
    load: async () => [...store.values()],
    put: async (e: OutboxEntry) => {
      store.set(e.id, { ...e });
    },
    remove: async (id: string) => {
      store.delete(id);
    },
  };
  let tick = 0;
  const outbox = createOutbox({
    storage,
    send: async (e) => {
      sent.push(e.id);
      return result();
    },
    now: () => `2026-01-01T00:00:${String(tick++).padStart(2, "0")}Z`,
  });
  return { outbox, sent, store };
}
const e = (id: string, createdAt: string) => ({
  id,
  kind: "action" as const,
  payload: { id },
  createdAt,
});

it("enqueue stores the entry and sends nothing while the connection is not open", async () => {
  const { outbox, sent, store } = setup();
  await outbox.onConnection("closed");
  await outbox.enqueue(e("a", "2026-01-01T00:00:00Z"));
  expect(sent).toEqual([]);
  expect(store.get("a")?.state).toBe("queued");
  expect(outbox.list().map((x) => x.id)).toEqual(["a"]);
});
it("opening the connection flushes queued entries in order", async () => {
  const { outbox, sent } = setup();
  await outbox.enqueue(e("late", "2026-01-01T00:00:09Z"));
  await outbox.enqueue(e("early", "2026-01-01T00:00:01Z"));
  await outbox.onConnection("open");
  expect(sent).toEqual(["early", "late"]);
});
it("a replayed entry keeps its id", async () => {
  const { outbox, sent } = setup();
  await outbox.enqueue(e("keep-me", "2026-01-01T00:00:00Z"));
  await outbox.onConnection("open");
  expect(sent).toEqual(["keep-me"]);
});
it("an ok result removes the entry", async () => {
  const { outbox, store } = setup();
  await outbox.enqueue(e("a", "2026-01-01T00:00:00Z"));
  await outbox.onConnection("open");
  expect(outbox.list()).toEqual([]);
  expect(store.has("a")).toBe(false);
});
it("a not-ok result marks the entry failed with the error", async () => {
  const { outbox, store } = setup([], () => ({ ok: false, error: "boom" }));
  await outbox.enqueue(e("a", "2026-01-01T00:00:00Z"));
  await outbox.onConnection("open");
  expect(outbox.list()[0]).toMatchObject({
    id: "a",
    state: "failed",
    error: "boom",
  });
  expect(store.get("a")?.state).toBe("failed");
});
it("a failed entry stays until discarded", async () => {
  const { outbox, sent } = setup([], () => ({ ok: false, error: "boom" }));
  await outbox.enqueue(e("a", "2026-01-01T00:00:00Z"));
  await outbox.onConnection("open");
  await outbox.flush();
  expect(sent).toEqual(["a"]);
  expect(outbox.list()).toHaveLength(1);
  await outbox.discard("a");
  expect(outbox.list()).toEqual([]);
});
it("an entry already sending is not sent twice", async () => {
  const { outbox, sent } = setup();
  await outbox.enqueue(e("a", "2026-01-01T00:00:00Z"));
  await outbox.onConnection("open");
  await Promise.all([outbox.flush(), outbox.flush()]);
  expect(sent).toEqual(["a"]);
});
it("entries are restored from storage on creation", async () => {
  const { outbox } = setup([
    { ...e("r", "2026-01-01T00:00:00Z"), state: "queued" },
  ]);
  await outbox.flush();
  expect(outbox.list().map((x) => x.id)).toEqual(["r"]);
});
it("an entry sending when the socket dropped is resent with the same id", async () => {
  const { outbox, sent } = setup([
    { ...e("s", "2026-01-01T00:00:00Z"), state: "sending" },
  ]);
  await outbox.onConnection("open");
  expect(sent).toEqual(["s"]);
});

it.each(["reject", "throw"])("keeps delivery working when storage load fails (%s)", async (failure) => {
  const sent: string[] = [];
  const outbox = createOutbox({ storage: {
    load: () => { if (failure === "throw") throw new Error("denied"); return Promise.reject(new Error("denied")); },
    put: async () => { throw new Error("denied"); }, remove: async () => { throw new Error("denied"); },
  }, send: async (entry) => { sent.push(entry.id); return { ok: true }; }, now: () => "t" });
  await outbox.enqueue(e("offline", "t"));
  await outbox.onConnection("open");
  await outbox.enqueue(e("online", "u"));
  expect(sent).toEqual(["offline", "online"]);
  expect(outbox.list()).toEqual([]);
});

it("continues in memory after a persistent write fails", async () => {
  const outbox = createOutbox({ storage: { load: () => [], put: async () => { throw new Error("quota"); }, remove: async () => { throw new Error("quota"); } },
    send: async () => ({ ok: false, error: "retry me" }), now: () => "t" });
  await outbox.onConnection("open"); await outbox.enqueue(e("a", "t"));
  expect(outbox.list()).toMatchObject([{ id: "a", state: "failed" }]);
  await outbox.retry("a"); await outbox.discard("a");
  expect(outbox.list()).toEqual([]);
});

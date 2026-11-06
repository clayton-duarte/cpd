import { describe, expect, it } from "vitest";
import type { Completion } from "../protocol/domain.ts";
import { createCompletionStore } from "./completion-store.ts";

const done: Completion = { label: "safe_to_archive", text: "Safe to archive · 93%", score: 93, tone: "done" };
const rich: Completion = { ...done, others: [{ label: "open_work", score: 30 }] };

const fake = (reply: unknown = { ok: true }) => {
  const calls: Array<readonly string[]> = [];
  const cpd = (args: readonly string[]): Promise<unknown> => {
    calls.push(args);
    return Promise.resolve(reply);
  };
  return { calls, cpd };
};

const flag = (args: readonly string[], name: string): string | undefined => args[args.indexOf(name) + 1];

describe("completion store (cpd swr badge)", () => {
  it("get asks cpd for the session's badge and returns the entry id and the completion", async () => {
    const { calls, cpd } = fake({ ok: true, badge: { session_id: "s1", entry_id: "e1", label: "safe_to_archive", score: 93, text: done.text, tone: "done", synced_at: "t" } });
    const got = await createCompletionStore({ cpd }).get("s1");
    expect(calls).toEqual([["swr", "badge", "get", "s1"]]);
    expect(got).toEqual({ entryId: "e1", completion: done });
  });

  it("get returns null on a miss", async () => {
    const { cpd } = fake({ ok: true, badge: null });
    expect(await createCompletionStore({ cpd }).get("s1")).toBeNull();
  });

  it("get maps others back and leaves the key out when cpd omits it", async () => {
    const row = { session_id: "s1", entry_id: "e1", label: "safe_to_archive", score: 93, text: done.text, tone: "done", synced_at: "t" };
    const withOthers = await createCompletionStore({ cpd: fake({ ok: true, badge: { ...row, others: rich.others } }).cpd }).get("s1");
    expect(withOthers?.completion).toEqual(rich);
    const without = await createCompletionStore({ cpd: fake({ ok: true, badge: row }).cpd }).get("s1");
    expect(without?.completion).not.toHaveProperty("others");
  });

  it("put sends every field as a flag and no --others when the completion has none", async () => {
    const { calls, cpd } = fake();
    await createCompletionStore({ cpd }).put("s1", "e1", done);
    expect(calls).toHaveLength(1);
    const args = calls[0] ?? [];
    expect(args.slice(0, 4)).toEqual(["swr", "badge", "put", "s1"]);
    expect(flag(args, "--entry")).toBe("e1");
    expect(flag(args, "--label")).toBe("safe_to_archive");
    expect(flag(args, "--score")).toBe("93");
    expect(flag(args, "--text")).toBe(done.text);
    expect(flag(args, "--tone")).toBe("done");
    expect(args).not.toContain("--others");
  });

  it("put sends others as a JSON array", async () => {
    const { calls, cpd } = fake();
    await createCompletionStore({ cpd }).put("s1", "e1", rich);
    expect(JSON.parse(flag(calls[0] ?? [], "--others") ?? "null")).toEqual(rich.others);
  });

  it("put never sends an empty others array", async () => {
    const { calls, cpd } = fake();
    await createCompletionStore({ cpd }).put("s1", "e1", { ...done, others: [] });
    expect(calls[0]).not.toContain("--others");
  });

  it("a cpd failure rejects", async () => {
    const cpd = (): Promise<unknown> => Promise.reject(new Error("exit 5"));
    await expect(createCompletionStore({ cpd }).get("s1")).rejects.toThrow("exit 5");
  });
});

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DialogResponse, SessionRef } from "../../protocol/domain.ts";
import type { Disposable, GitBoundary } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { createPiBackend } from "./backend.ts";
import type { Cpd } from "../cockpit/cli.ts";
import type { SessionHandle } from "../hub/connection.ts";
import type { WatchBoundary } from "./watch.ts";
import type { FileBoundary } from "./window.ts";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const enc = new TextEncoder();
const FILE = "/s/s1.jsonl";
class FakeFile implements FileBoundary {
  bytes: Uint8Array;
  stats: string[] = [];
  constructor(text: string) {
    this.bytes = enc.encode(text);
  }
  stat(path: string): Promise<{ size: number; identity: string }> {
    this.stats.push(path);
    return Promise.resolve({ size: this.bytes.length, identity: "1:1" });
  }
  read(_p: string, start: number, end: number): Promise<Uint8Array> {
    return Promise.resolve(this.bytes.slice(start, end));
  }
  writes: Array<{ path: string; bytes: Uint8Array }> = [];
  write(path: string, bytes: Uint8Array): Promise<void> {
    this.writes.push({ path, bytes });
    return Promise.resolve();
  }
}
class FakeWatch implements WatchBoundary {
  disposed = 0;
  watch(): Disposable {
    return { dispose: () => void (this.disposed += 1) };
  }
}
class FakeCpd implements Cpd {
  calls: Array<readonly string[]> = [];
  show: Record<string, unknown> = { model: "bifrost/m1", thinking: "high", tier: "S3" };
  failShow = false;
  json(args: readonly string[]): Promise<Readonly<Record<string, unknown>>> {
    this.calls.push(args);
    if (args[1] === "show") {
      if (this.failShow) return Promise.reject(new Error("cpd down"));
      return Promise.resolve({ ok: true, session: this.show });
    }
    return Promise.resolve({ ok: true });
  }
}

const ref = (over: Partial<SessionRef> = {}): SessionRef => ({
  id: "s1",
  key: "ctx:plan:job",
  runner: "pi",
  role: "worker",
  state: "working",
  title: "My chat",
  file: FILE,
  tier: "S3",
  cwd: "/repo",
  ...over,
});

async function setup(opts: { text: string; attachmentsDir?: string; now?: number }) {
  const fs = new FakeFile(opts.text);
  const cpd = new FakeCpd();
  const watch = new FakeWatch();
  const git: GitBoundary = { branch: () => Promise.resolve("main") };
  const out: SessionServerMessage[] = [];
  const backend = createPiBackend({ cpd, fs, watch, git, cpdHome: "/home/cpd", attachmentsDir: opts.attachmentsDir ?? "/att", now: () => opts.now ?? 1000, scheduler: new FakeScheduler() });
  const handle: SessionHandle = await backend.attach({ ref: ref(), file: FILE, tierOptions: ["S1", "S2", "S3"] }, (m) => out.push(m));
  return { fs, cpd, out, handle };
}
const rejects = async (p: Promise<unknown>) => {
  try {
    await p;
  } catch (e) {
    return e as { code?: string; message: string };
  }
  throw new Error("did not reject");
};
const reviewLine = (n: string) =>
  JSON.stringify({ type: "custom", id: `r${n}`, customType: "cpd-plan-review", data: { review: n, status: "pending" } });
const REVIEW_TEXT = `${reviewLine("7")}\n`;
const ASK_TEXT = `${JSON.stringify({
  type: "message",
  id: "ask",
  message: {
    role: "assistant",
    content: [{ type: "toolCall", id: "q1", name: "ask_user_question", arguments: {
      questions: [{ header: "Choice", question: "Proceed?", options: [
        { label: "Yes", description: "Continue" }, { label: "No", description: "Stop" },
      ] }],
    } }],
  },
})}\n`;
const ANSWER: DialogResponse = { method: "ask_user_question", outcome: "answered", result: {
  answers: [{ questionIndex: 0, question: "Proceed?", kind: "option", answer: "Yes" }],
  cancelled: false,
} };
const CANCEL: DialogResponse = { method: "ask_user_question", outcome: "cancelled" };
const decide = (n: string, flag: string[]) => ["plan", "decide", n, ...flag, "--plan", "plan"];

describe("pi backend plan review dialogs", () => {
  it.each([ANSWER, CANCEL])("refuses an unknown $outcome question ID without touching the pending question", async (response) => {
    const s = await setup({ text: ASK_TEXT });
    await expect(s.handle.respond("call:missing", response)).rejects.toMatchObject({ code: "invalid_message" });
    expect(s.fs.writes).toEqual([]);
    expect(s.out.filter((m) => m.type === "session.dialogResolved")).toEqual([]);
    await s.handle.respond("call:q1", response);
    expect(s.fs.writes).toHaveLength(1);
    await s.handle.dispose();
  });

  it.each([ANSWER, CANCEL])("refuses a replayed $outcome question response", async (response) => {
    const s = await setup({ text: ASK_TEXT });
    await s.handle.respond("call:q1", response);
    await expect(s.handle.respond("call:q1", response)).rejects.toMatchObject({ code: "invalid_message" });
    expect(s.fs.writes).toHaveLength(1);
    expect(s.out.filter((m) => m.type === "session.dialogResolved")).toHaveLength(1);
    await s.handle.dispose();
  });

  it.each([ANSWER, CANCEL])("refuses a duplicate $outcome while delivery is in flight", async (response) => {
    const s = await setup({ text: ASK_TEXT });
    let finishWrite!: () => void;
    const delivery = new Promise<void>((resolve) => { finishWrite = resolve; });
    const write = vi.spyOn(s.fs, "write").mockReturnValueOnce(delivery);
    const first = s.handle.respond("call:q1", response);
    try {
      await expect(s.handle.respond("call:q1", response)).rejects.toMatchObject({ code: "invalid_message" });
      expect(write).toHaveBeenCalledTimes(1);
      expect(s.out.filter((m) => m.type === "session.dialogResolved")).toEqual([]);
    } finally {
      finishWrite();
      await first;
      await s.handle.dispose();
    }
  });

  it("refuses mismatched methods and unknown review cancellations before effects", async () => {
    const s = await setup({ text: `${ASK_TEXT}${REVIEW_TEXT}` });
    await expect(s.handle.respond("review:7", ANSWER)).rejects.toMatchObject({ code: "invalid_message" });
    for (const dialogId of ["call:q1", "review:missing"]) {
      await expect(s.handle.respond(dialogId, { method: "cpd_plan_review", outcome: "cancelled" })).rejects.toMatchObject({ code: "invalid_message" });
    }
    expect(s.fs.writes).toEqual([]);
    expect(s.out.filter((m) => m.type === "session.dialogResolved")).toEqual([]);
    expect(s.cpd.calls.some((args) => args[1] === "decide")).toBe(false);
    await s.handle.respond("call:q1", ANSWER);
    await s.handle.respond("review:7", { method: "cpd_plan_review", outcome: "cancelled" });
    await s.handle.dispose();
  });

  it.each([ANSWER, CANCEL])("keeps $outcome unresolved while the response write is pending", async (response) => {
    const s = await setup({ text: ASK_TEXT });
    expect(s.out).toContainEqual(expect.objectContaining({ type: "session.dialog", dialog: expect.objectContaining({ id: "call:q1" }) }));
    let finishWrite!: () => void;
    const delivery = new Promise<void>((resolve) => { finishWrite = resolve; });
    vi.spyOn(s.fs, "write").mockReturnValueOnce(delivery);
    const responded = s.handle.respond("call:q1", response);
    try {
      expect(s.out.filter((m) => m.type === "session.dialogResolved")).toEqual([]);
    } finally {
      finishWrite();
      await responded;
    }
    expect(s.out.filter((m) => m.type === "session.dialogResolved")).toEqual([
      { type: "session.dialogResolved", sessionId: "s1", dialogId: "call:q1", outcome: response.outcome },
    ]);
    await s.handle.dispose();
  });

  it.each([ANSWER, CANCEL])("keeps failed $outcome delivery retryable without premature resolution", async (response) => {
    const s = await setup({ text: ASK_TEXT });
    vi.spyOn(s.fs, "write").mockRejectedValueOnce(new Error("disk full"));
    await expect(s.handle.respond("call:q1", response)).rejects.toThrow("disk full");
    expect(s.out.filter((m) => m.type === "session.dialogResolved")).toEqual([]);
    expect(s.fs.writes).toEqual([]);

    await s.handle.respond("call:q1", response);
    expect(s.fs.writes).toHaveLength(1);
    expect(JSON.parse(new TextDecoder().decode(s.fs.writes[0]?.bytes))).toEqual({
      dialogId: "call:q1", result: response.outcome === "answered" ? response.result : { answers: [], cancelled: true },
    });
    expect(s.out.filter((m) => m.type === "session.dialogResolved")).toEqual([
      { type: "session.dialogResolved", sessionId: "s1", dialogId: "call:q1", outcome: response.outcome },
    ]);
    await s.handle.dispose();
  });

  it("a cancelled ask_user_question answer resolves cancelled and goes to the ask answer file", async () => {
    const s = await setup({ text: ASK_TEXT });
    await s.handle.respond("call:q1", { method: "ask_user_question", outcome: "cancelled" });
    expect(s.out).toContainEqual({ type: "session.dialogResolved", sessionId: "s1", dialogId: "call:q1", outcome: "cancelled" });
    const write = s.fs.writes.find((w) => w.path === "/home/cpd/run/ask/s1.json");
    expect(JSON.parse(new TextDecoder().decode(write?.bytes))).toEqual({ dialogId: "call:q1", result: { answers: [], cancelled: true } });
  });

  it("the pi backend declares interactive dialogs", async () => {
    const s = await setup({ text: REVIEW_TEXT });
    expect(s.handle.capabilities.dialogs).toEqual({ mode: "interactive" });
  });

  it("approve runs cpd plan decide approve for the review and plan", async () => {
    const s = await setup({ text: REVIEW_TEXT });
    await s.handle.respond("review:7", { method: "cpd_plan_review", outcome: "approve" });
    expect(s.cpd.calls).toContainEqual(decide("7", ["--approve"]));
    expect(s.out.some((m) => m.type === "session.dialogResolved")).toBe(false);
  });

  it("reject runs cpd plan decide reject for the review and plan", async () => {
    const s = await setup({ text: REVIEW_TEXT });
    await s.handle.respond("review:7", { method: "cpd_plan_review", outcome: "reject" });
    expect(s.cpd.calls).toContainEqual(decide("7", ["--reject"]));
  });

  it("changes writes the feedback file and runs cpd plan decide reject with it", async () => {
    const s = await setup({ text: REVIEW_TEXT });
    await s.handle.respond("review:7", { method: "cpd_plan_review", outcome: "changes", feedback: "rename the step" });
    const path = "/home/cpd/run/plan/feedback-7.md";
    expect(s.fs.writes.map((w) => [w.path, new TextDecoder().decode(w.bytes)])).toEqual([[path, "rename the step"]]);
    expect(s.cpd.calls).toContainEqual(["plan", "decide", "7", "--reject", "--feedback-file", path, "--plan", "plan"]);
  });

  it("a response for a dialog that is not a pending review is refused invalid_message", async () => {
    const s = await setup({ text: REVIEW_TEXT });
    const err = await rejects(s.handle.respond("review:99", { method: "cpd_plan_review", outcome: "approve" }));
    expect(err.code).toBe("invalid_message");
    await s.handle.respond("review:7", { method: "cpd_plan_review", outcome: "cancelled" });
    expect(s.cpd.calls.some((c) => c[1] === "decide")).toBe(false);
  });
});

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ stat: vi.fn(), rm: vi.fn(), execFile: vi.fn() }));
vi.mock("node:fs/promises", () => ({ stat: mocks.stat, rm: mocks.rm }));
vi.mock("node:child_process", () => ({ execFile: mocks.execFile }));

import type { DialogResponse, SessionRef } from "../../protocol/domain.ts";
import type { Disposable, GitBoundary } from "../../protocol/runtime.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { createPiBackend } from "./backend.ts";
import type { Cpd } from "../cockpit/cli.ts";
import type { WatchBoundary } from "./watch.ts";
import type { FileBoundary } from "./window.ts";

beforeEach(() => {
  vi.useFakeTimers();
  mocks.stat.mockReset().mockResolvedValue({});
  mocks.rm.mockReset().mockResolvedValue(undefined);
  mocks.execFile.mockReset().mockImplementation((_c: string, _a: string[], cb: () => void) => cb());
});
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

const enc = new TextEncoder();
const FILE = "/s/s1.jsonl";
const SECRET = "SECRET-custom-answer-text";
const ASK_TEXT = `${JSON.stringify({
  type: "message",
  id: "ask",
  message: { role: "assistant", content: [{ type: "toolCall", id: "q1", name: "ask_user_question", arguments: {
    questions: [{ header: "Choice", question: "Proceed?", options: [{ label: "Yes", description: "Continue" }, { label: "No", description: "Stop" }] }],
  } }] },
})}\n`;
const ANSWER: DialogResponse = { method: "ask_user_question", outcome: "answered", result: {
  answers: [{ questionIndex: 0, question: "Proceed?", kind: "custom", answer: SECRET }],
  cancelled: false,
} };

const fakeFs = (): FileBoundary & { writes: string[] } => {
  const bytes = enc.encode(ASK_TEXT);
  const writes: string[] = [];
  return {
    writes,
    stat: () => Promise.resolve({ size: bytes.length, identity: "1:1" }),
    read: (_p, a, b) => Promise.resolve(bytes.slice(a, b)),
    write: (path) => { writes.push(path); return Promise.resolve(); },
  };
};
const cpd: Cpd = { json: (args) => Promise.resolve(args[1] === "show" ? { ok: true, session: { tmux_pane: "%7" } } : { ok: true }) };
const watch: WatchBoundary = { watch: (): Disposable => ({ dispose: () => undefined }) };
const git: GitBoundary = { branch: () => Promise.resolve("main") };
const ref: SessionRef = { id: "s1", key: "ctx:plan:job", runner: "pi", role: "worker", state: "working", title: "t", file: FILE, tier: "S3", cwd: "/repo" };

async function setup(diag: (r: Record<string, unknown>) => void) {
  const backend = createPiBackend({
    cpd, fs: fakeFs(), watch, git, cpdHome: "/home/cpd", attachmentsDir: "/att", now: () => Date.UTC(2026, 0, 15, 12), scheduler: new FakeScheduler(), diag,
  });
  return backend.attach({ ref, file: FILE, tierOptions: ["S1", "S2", "S3"] }, () => undefined);
}
const events = (rs: Record<string, unknown>[]) => rs.map((r) => r.event);

describe("ask answer diagnostics (web)", () => {
  it("respond records received and written without answer text", async () => {
    const rs: Record<string, unknown>[] = [];
    const h = await setup((r) => rs.push(r));
    await h.respond("call:q1", ANSWER);
    expect(events(rs)).toEqual(["respond.received", "respond.written"]);
    expect(rs[0]).toMatchObject({
      side: "web", event: "respond.received", session: "s1", dialogId: "call:q1", at: "2026-01-15T12:00:00.000Z",
      method: "ask_user_question", outcome: "answered",
      answers: [{ kind: "custom", answerLength: SECRET.length }],
    });
    expect(rs[1]).toMatchObject({ side: "web", event: "respond.written", session: "s1", dialogId: "call:q1", path: "/home/cpd/run/ask/s1.json" });
    expect(JSON.stringify(rs)).not.toContain(SECRET);
    await h.dispose();
  });

  it("respond records refused with its reason", async () => {
    const rs: Record<string, unknown>[] = [];
    const h = await setup((r) => rs.push(r));
    await expect(h.respond("call:missing", ANSWER)).rejects.toMatchObject({ code: "invalid_message" });
    expect(events(rs)).toContain("respond.refused");
    const refused = rs.find((r) => r.event === "respond.refused");
    expect(refused).toMatchObject({ side: "web", session: "s1", dialogId: "call:missing" });
    expect(typeof refused?.reason).toBe("string");
    expect(String(refused?.reason).length).toBeGreaterThan(0);
    expect(events(rs)).not.toContain("respond.written");
    await h.dispose();
  });

  it("respond records escape_sent when pi did not consume the file", async () => {
    const rs: Record<string, unknown>[] = [];
    const h = await setup((r) => rs.push(r));
    await h.respond("call:q1", ANSWER);
    expect(events(rs)).not.toContain("respond.escape_sent");
    await vi.advanceTimersByTimeAsync(350);
    expect(mocks.execFile).toHaveBeenCalledWith("tmux", ["send-keys", "-t", "%7", "Escape"], expect.any(Function));
    expect(events(rs)).toContain("respond.escape_sent");
    expect(rs.find((r) => r.event === "respond.escape_sent")).toMatchObject({ side: "web", session: "s1", dialogId: "call:q1" });
    expect(JSON.stringify(rs)).not.toContain(SECRET);
    await h.dispose();
  });

  it("a throwing diagnostic sink does not change the respond result", async () => {
    const calls = vi.fn(() => { throw new Error("sink down"); });
    const h = await setup(calls);
    await expect(h.respond("call:q1", ANSWER)).resolves.toBeUndefined();
    expect(calls).toHaveBeenCalled();
    await expect(h.respond("call:missing", ANSWER)).rejects.toMatchObject({ code: "invalid_message" });
    await vi.advanceTimersByTimeAsync(350);
    await h.dispose();
  });
});

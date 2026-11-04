import { describe, expect, it } from "vitest";
import type { FooterStats, SessionRef } from "../../protocol/domain.ts";
import { PI_SEND_TEXT_MAX } from "../../protocol/domain.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { ServerMessage, SessionServerMessage } from "../../protocol/wire.ts";
import { createPiBackend } from "./backend.ts";
import type { GitBoundary } from "../../protocol/runtime.ts";
import type { Cpd } from "../cockpit/cli.ts";
import type { SessionHandle } from "../hub/connection.ts";
import type { WatchBoundary } from "./watch.ts";
import type { FileBoundary } from "./window.ts";

const enc = new TextEncoder();
const FILE = "/s/s1.jsonl";
const userLine = (i: number): string =>
  JSON.stringify({ type: "message", id: `m${i}`, message: { role: "user", content: `message ${i}` } });
const askCall = JSON.stringify({
  type: "message",
  id: "a1",
  message: {
    role: "assistant",
    stopReason: "toolUse",
    content: [
      {
        type: "toolCall",
        id: "q1",
        name: "ask_user_question",
        arguments: {
          questions: [
            {
              header: "Verdict",
              question: "Approve?",
              options: [
                { label: "Yes", description: "go" },
                { label: "No", description: "stop" },
              ],
            },
          ],
        },
      },
    ],
  },
});

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
  write(): Promise<void> {
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

async function setup(opts: { text?: string; ref?: Partial<SessionRef>; branch?: string | null } = {}) {
  const fs = new FakeFile(opts.text ?? `${userLine(0)}\n`);
  const cpd = new FakeCpd();
  const watch = new FakeWatch();
  const gitCalls: string[] = [];
  const git: GitBoundary = {
    branch: (cwd) => {
      gitCalls.push(cwd);
      return Promise.resolve(opts.branch === undefined ? "main" : opts.branch);
    },
  };
  const out: SessionServerMessage[] = [];
  const backend = createPiBackend({ cpd, fs, watch, git, cpdHome: "/home", attachmentsDir: "/att", now: () => 1000 });
  const attach = () => backend.attach({ ref: ref(opts.ref), file: opts.ref && "file" in opts.ref ? opts.ref.file : FILE, tierOptions: ["S1", "S2", "S3"] }, (m) => out.push(m));
  return { fs, cpd, watch, gitCalls, out, attach };
}
const types = (out: ServerMessage[]): string[] => out.map((m) => m.type);
const find = <T extends ServerMessage["type"]>(out: ServerMessage[], type: T) =>
  out.find((m) => m.type === type) as Extract<ServerMessage, { type: T }>;
const rejects = async (p: Promise<unknown>) => {
  try {
    await p;
  } catch (e) {
    return e as { code?: string; message: string };
  }
  throw new Error("did not reject");
};
const footerOf = (out: ServerMessage[]): FooterStats => find(out, "session.footer").footer;
const attached = async (opts: Parameters<typeof setup>[0] = {}) => {
  const s = await setup(opts);
  const handle: SessionHandle = await s.attach();
  return { ...s, handle };
};

describe("pi backend", () => {
  it("attach resolves the session file from the cpd session row", async () => {
    const s = await attached();
    expect(s.fs.stats[0]).toBe(FILE);
    expect(s.cpd.calls).toContainEqual(["session", "show", "s1"]);
    expect(types(s.out).slice(0, 4)).toEqual(["session.attached", "session.window", "session.footer", "session.status"]);
    const a = find(s.out, "session.attached");
    expect(a.sessionId).toBe("s1");
    expect(a.title).toBe("My chat");
    expect(a.meta.ref.id).toBe("s1");
    const w = find(s.out, "session.window");
    expect(w.mode).toBe("tail");
    expect(w.window.items.map((i) => i.id)).toEqual(["u:m0"]);
    expect(find(s.out, "session.status").queued).toEqual([]);
    const missing = await setup({ ref: { file: undefined } });
    expect(await rejects(missing.attach())).toMatchObject({ code: "session_file_missing" });
    expect(missing.out).toEqual([]);
    const down = await setup();
    down.cpd.failShow = true;
    await down.attach();
    const f = footerOf(down.out);
    expect(f.model.state).toBe("unknown");
    expect(f.tier).toEqual({ state: "known", value: "S3" });
    await s.handle.dispose();
    expect(s.watch.disposed).toBe(1);
  });

  it("send maps to cpd session send with the text", async () => {
    const s = await attached();
    await s.handle.submit({ clientSubmissionId: "c1", text: "-hello there", images: [], whenBusy: "followUp" });
    expect(s.cpd.calls).toContainEqual(["session", "send", "s1", "--unblocks", "User message from the web chat", "--", "-hello there"]);
    const ack = s.out.find((m) => m.type === "session.submitAck");
    expect(ack).toMatchObject({ sessionId: "s1", clientSubmissionId: "c1", queued: false });
    const sends = () => s.cpd.calls.filter((c) => c[1] === "send").length;
    const before = sends();
    expect(await rejects(s.handle.submit({ clientSubmissionId: "c2", text: "   ", images: [], whenBusy: "followUp" }))).toMatchObject({ code: "invalid_submission" });
    expect(await rejects(s.handle.submit({ clientSubmissionId: "c3", text: "x".repeat(PI_SEND_TEXT_MAX + 1), images: [], whenBusy: "followUp" }))).toMatchObject({ code: "invalid_submission" });
    const steer = await rejects(s.handle.submit({ clientSubmissionId: "c4", text: "go", images: [], whenBusy: "steer" }));
    expect(steer).toMatchObject({ code: "unsupported_capability", message: expect.stringMatching(/\S/) });
    expect(sends()).toBe(before);
  });

  it("text file attachments are inlined and images are declared supported", async () => {
    const s = await attached();
    const inlined = 'look at this\n<file name="notes.txt">\nline one\n</file>';
    await s.handle.submit({ clientSubmissionId: "c1", text: inlined, images: [], whenBusy: "followUp" });
    expect(s.cpd.calls.at(-1)).toEqual(["session", "send", "s1", "--unblocks", "User message from the web chat", "--", inlined]);
    expect(s.handle.capabilities.images.supported).toBe(true);
  });

  it("retier maps to cpd session retier with the tier", async () => {
    const s = await attached();
    s.cpd.show = { model: "bifrost/m2", thinking: "low", tier: "S1" };
    const before = s.out.length;
    await s.handle.configure({ kind: "tier", tier: "S1" });
    const idx = s.cpd.calls.findIndex((c) => c[1] === "retier");
    expect(s.cpd.calls[idx]).toEqual(["session", "retier", "s1", "--tier", "S1"]);
    expect(s.cpd.calls.slice(idx + 1)).toContainEqual(["session", "show", "s1"]);
    const footers = s.out.slice(before).filter((m) => m.type === "session.footer");
    expect(footers).toHaveLength(1);
    expect(footerOf(s.out.slice(before))).toMatchObject({ tier: { state: "known", value: "S1" } });
    const model = await rejects(s.handle.configure({ kind: "model", model: "a/b" }));
    expect(model.code).toBe("unsupported_capability");
    expect((await rejects(s.handle.configure({ kind: "thinking", level: "low" }))).code).toBe("unsupported_capability");
    expect(s.handle.capabilities.configure.tier).toMatchObject({ supported: true, options: ["S1", "S2", "S3"] });
  });

  it("abort and steer are declared unsupported with a reason", async () => {
    const s = await attached();
    const c = s.handle.capabilities;
    for (const cap of [c.steer, c.abort, c.abortSubmission, c.compact, c.configure.model, c.configure.thinking]) {
      expect(cap.supported).toBe(false);
      if (!cap.supported) expect(cap.reason).toMatch(/\S/);
    }
    expect(c.followUp.supported).toBe(true);
    expect(c.dialogs.mode).toBe("interactive");
    expect(c.images.maxCount).toBeGreaterThan(0);
    const calls = [
      s.handle.abort(),
      s.handle.abortSubmission("c1"),
      s.handle.compact(),
      s.handle.listModels(),
      s.handle.respond("call:q1", { method: "ask_user_question", outcome: "cancelled" }),
    ];
    for (const p of calls) expect((await rejects(p)).code).toBe("unsupported_capability");
    expect(find(s.out, "session.attached").capabilities.abort.supported).toBe(false);
  });

  it("dialogs are read-only reconstructions from tool calls", async () => {
    const s = await attached({ text: `${userLine(0)}\n${askCall}\n` });
    const dialogs = s.out.filter((m) => m.type === "session.dialog");
    expect(dialogs).toHaveLength(1);
    const d = (dialogs[0] as Extract<ServerMessage, { type: "session.dialog" }>).dialog;
    expect(d).toMatchObject({ id: "call:q1", method: "ask_user_question", interactive: false });
    expect(d.method === "ask_user_question" && d.questions[0]?.question).toBe("Approve?");
    expect(types(s.out).indexOf("session.dialog")).toBeGreaterThan(types(s.out).indexOf("session.status"));
    const none = await attached();
    expect(none.out.some((m) => m.type === "session.dialog")).toBe(false);
  });

  it("footer reports the git branch of the session cwd", async () => {
    const s = await attached();
    expect(s.gitCalls).toEqual(["/repo"]);
    const f = footerOf(s.out);
    expect(f.branch).toEqual({ state: "known", value: "main" });
    expect(f.model).toEqual({ state: "known", value: "bifrost/m1" });
    expect(f.thinking).toEqual({ state: "known", value: "high" });
    expect(f.cost.state).toBe("unknown");
    expect(f.contextPercent.state).toBe("unknown");
    const none = await attached({ branch: null });
    const nb = footerOf(none.out).branch;
    expect(nb.state).toBe("unknown");
    if (nb.state === "unknown") expect(nb.reason).toMatch(/\S/);
    const nocwd = await attached({ ref: { cwd: undefined } });
    expect(nocwd.gitCalls).toEqual([]);
    expect(footerOf(nocwd.out).branch.state).toBe("unknown");
    const withModelChange = await attached({
      text: `${JSON.stringify({ type: "model_change", id: "mc", provider: "p", modelId: "later" })}\n${userLine(0)}\n`,
    });
    expect(footerOf(withModelChange.out).model).toEqual({ state: "known", value: "p/later" });
  });
});

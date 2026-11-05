import { describe, expect, it } from "vitest";
import type { SessionRef, TranscriptItem } from "../../protocol/domain.ts";
import type { Disposable, GitBoundary } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import type { Cpd } from "../cockpit/cli.ts";
import { createPiBackend } from "./backend.ts";
import type { LiveSource } from "./live.ts";
import type { WatchBoundary } from "./watch.ts";
import type { FileBoundary } from "./window.ts";

const enc = new TextEncoder();
const FILE = "/s/s1.jsonl";

class FakeFile implements FileBoundary {
  bytes: Uint8Array;
  constructor(text: string) {
    this.bytes = enc.encode(text);
  }
  append(text: string): void {
    const add = enc.encode(text);
    const next = new Uint8Array(this.bytes.length + add.length);
    next.set(this.bytes);
    next.set(add, this.bytes.length);
    this.bytes = next;
  }
  stat(): Promise<{ size: number; identity: string }> {
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
  private changes: Array<() => void> = [];
  watch(_path: string, onChange: () => void): Disposable {
    this.changes.push(onChange);
    return { dispose: () => void (this.disposed += 1) };
  }
  fire(): void {
    for (const c of this.changes) c();
  }
}
class FakeCpd implements Cpd {
  json(args: readonly string[]): Promise<Readonly<Record<string, unknown>>> {
    return Promise.resolve(args[1] === "show" ? { ok: true, session: { model: "m/1", thinking: "high", tier: "S3" } } : { ok: true });
  }
}
class FakeLive implements LiveSource {
  attached: string[] = [];
  detached = 0;
  emit: (m: SessionServerMessage) => void = () => {};
  committed: (ts: number) => boolean = () => false;
  attach(sessionId: string, emit: (m: SessionServerMessage) => void, committed: (ts: number) => boolean): void {
    this.attached.push(sessionId);
    this.emit = emit;
    this.committed = committed;
  }
  detach(): void {
    this.detached += 1;
  }
}

const userLine = JSON.stringify({ type: "message", id: "u0", message: { role: "user", content: "hello" } });
const assistantLine = (id: string, ts: number, content: unknown[]): string =>
  JSON.stringify({ type: "message", id, message: { role: "assistant", timestamp: ts, stopReason: "stop", content } });
const text = (t: string) => ({ type: "text", text: t });
const live = (ts: number): TranscriptItem => ({ kind: "assistant", id: `live:${ts}`, blocks: [text("so far")], streaming: true });
const liveTool = (callId: string): TranscriptItem => ({ kind: "tool", id: `tool:${callId}`, callId, name: "read", args: {}, status: "running" });

const ref: SessionRef = { id: "s1", key: "c:p:j", runner: "pi", role: "worker", state: "working", title: "T", file: FILE, tier: "S3", cwd: "/repo" };
const flush = async (): Promise<void> => {
  for (let i = 0; i < 50; i++) await Promise.resolve();
};

async function setup(initial: string[], withLive = true) {
  const fs = new FakeFile(initial.map((l) => `${l}\n`).join(""));
  const watch = new FakeWatch();
  const source = new FakeLive();
  const git: GitBoundary = { branch: () => Promise.resolve("main") };
  const out: SessionServerMessage[] = [];
  const backend = createPiBackend({
    cpd: new FakeCpd(),
    fs,
    watch,
    git,
    cpdHome: "/home",
    attachmentsDir: "/att",
    now: () => 1000,
    ...(withLive ? { live: source } : {}),
  });
  const handle = await backend.attach({ ref, file: FILE, tierOptions: ["S3"] }, (m) => out.push(m));
  const commit = async (...lines: string[]): Promise<void> => {
    out.length = 0;
    fs.append(lines.map((l) => `${l}\n`).join(""));
    watch.fire();
    await flush();
  };
  return { fs, watch, source, out, handle, commit };
}

/** Applies append, update and remove the way the client does. */
function apply(items: TranscriptItem[], m: SessionServerMessage): TranscriptItem[] {
  if (m.type === "session.append" || m.type === "session.update") {
    const next = [...items];
    for (const i of m.items) {
      const at = next.findIndex((x) => x.id === i.id);
      if (at >= 0) next[at] = i;
      else next.push(i);
    }
    return next;
  }
  if (m.type === "session.remove") return items.filter((i) => i.id !== m.itemId);
  return items;
}

describe("pi backend with a live source", () => {
  it("committed assistant line removes the live item then appends the committed item", async () => {
    const s = await setup([userLine]);
    expect(s.source.attached).toEqual(["s1"]);
    s.source.emit({ type: "session.append", sessionId: "s1", items: [live(100)] });
    await s.commit(assistantLine("e1", 100, [text("final")]));
    const kinds = s.out.map((m) => (m.type === "session.remove" ? `remove ${m.itemId}` : m.type === "session.append" ? `append ${m.items.map((i) => i.id).join()}` : m.type));
    const remove = kinds.indexOf("remove live:100");
    const append = kinds.indexOf("append a:e1");
    expect(remove).toBeGreaterThanOrEqual(0);
    expect(append).toBeGreaterThan(remove);
    expect(s.out.find((m) => m.type === "session.remove")).toEqual({ type: "session.remove", sessionId: "s1", itemId: "live:100" });
  });

  it("frames for an already committed timestamp are ignored", async () => {
    const s = await setup([userLine, assistantLine("e1", 100, [text("old")])]);
    expect(s.source.committed(100)).toBe(true);
    expect(s.source.committed(200)).toBe(false);
    await s.commit(assistantLine("e2", 200, [text("new")]));
    expect(s.source.committed(200)).toBe(true);
    await s.handle.dispose();
    expect(s.source.detached).toBe(1);
  });

  it("a committed tool item replaces the running tool item with no duplicate", async () => {
    const s = await setup([userLine]);
    s.source.emit({ type: "session.append", sessionId: "s1", items: [live(100), liveTool("c1")] });
    let items: TranscriptItem[] = [live(100), liveTool("c1")];
    await s.commit(assistantLine("e1", 100, [text("let me look"), { type: "toolCall", id: "c1", name: "read", arguments: { path: "a" } }]));
    for (const m of s.out) items = apply(items, m);
    expect(items.map((i) => i.id).sort()).toEqual(["a:e1", "tool:c1"]);
    expect(s.out.some((m) => m.type === "session.remove" && m.itemId === "live:100")).toBe(true);
    const toolResult = JSON.stringify({ type: "message", id: "r1", message: { role: "toolResult", toolCallId: "c1", toolName: "read", isError: false, content: [text("body")] } });
    await s.commit(toolResult);
    for (const m of s.out) items = apply(items, m);
    expect(items.filter((i) => i.id === "tool:c1")).toHaveLength(1);
    expect(items.find((i) => i.id === "tool:c1")).toMatchObject({ status: "done", result: { text: "body", isError: false } });
  });

  it("with no socket the backend emits exactly what it does today", async () => {
    const s = await setup([userLine]);
    expect(s.source.attached).toEqual(["s1"]);
    expect(s.out.map((m) => m.type)).toEqual(["session.attached", "session.window", "session.footer", "session.status"]);
    await s.commit(assistantLine("e1", 100, [text("final")]));
    expect(s.out.map((m) => m.type)).toEqual(["session.append", "session.status"]);
    expect(s.out.some((m) => m.type === "session.remove")).toBe(false);
  });
});

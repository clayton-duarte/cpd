import { createStore } from "../../state/store.ts";
import { createLiveSource } from "./live.ts";
import { expect, it } from "vitest";
import { createCompletionService } from "../completion-service.ts";
import type { CompletionPayload } from "../completion-classify.ts";
import type { PlanViewModel } from "../../protocol/cockpit.ts";
import type { SessionRef } from "../../protocol/domain.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import type { Cpd } from "../cockpit/cli.ts";
import { createPiBackend } from "./backend.ts";
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

const ref: SessionRef = { id: "s1", key: "c:p", runner: "pi", role: "lead", state: "working", title: "T", file: FILE, tier: "S3", cwd: "/repo" };
const text = (value: string) => ({ type: "text", text: value });
const line = (id: string, message: object) => JSON.stringify({ type: "message", id, message });
const flush = async () => { for (let i = 0; i < 50; i++) await Promise.resolve(); };
async function setupCompletion(lines: string[]) {
  const fs = new FakeFile(lines.join("\n") + "\n");
  const watch = new FakeWatch();
  const scheduler = new FakeScheduler();
  const payloads: CompletionPayload[] = [];
  const service = createCompletionService({ classifier: { classify: async (p) => { payloads.push(p); return null; } },
    planOf: async () => ({ planId: "p", contextId: "c", title: "P", phase: "execute", counts: {}, review: null, openEscalations: 0, workflows: [], jobs: [], cursor: 1 }) as unknown as PlanViewModel });
  const tracker = service.track(ref, () => {});
  const store = createStore();
  store.dispatch({ type: "selectSession", sessionId: ref.id });
  let feed: (value: string) => void = () => {};
  const live = createLiveSource({ cpdHome: "/home", scheduler, connect: () => ({ onLine: (cb) => { feed = cb; }, onClose: () => {}, close: () => {} }) });
  const backend = createPiBackend({ cpd: new FakeCpd(), fs, watch, git: { branch: async () => "main" }, cpdHome: "/home", attachmentsDir: "/att", now: () => 1000, scheduler, live });
  const messages: SessionServerMessage[] = [];
  const handle = await backend.attach({ ref, file: FILE, tierOptions: ["S3"] }, (message) => { messages.push(message); tracker.observe(message); store.dispatch({ type: "server", message }); });
  await flush();
  return { fs, watch, payloads, messages, handle, store, tracker, feed: (value: object) => feed(JSON.stringify(value)) };
}
it("completion payload has no removed live assistant after transcript reconciliation", async () => {
  const s = await setupCompletion([line("u1", { role: "user", content: "finish the work" })]);
  s.feed({ t: "hello", protocol: 1 });
  s.feed({ t: "message_start", message: { role: "assistant", timestamp: 100, content: [] } });
  s.feed({ t: "delta", kind: "text", phase: "delta", index: 0, delta: "finished" });
  s.fs.append(line("a1", { role: "assistant", timestamp: 100, stopReason: "stop", content: [text("finished")] }) + "\n");
  s.watch.fire();
  await flush();
  const current = s.store.getState().session!.transcript.items;
  expect(current.map((i) => i.kind)).toEqual(["user", "assistant"]);
  expect(s.payloads.at(-1)!.transcript).toEqual([{ kind: "user", text: "finish the work" }, { kind: "assistant", text: "finished" }]);
  s.tracker.dispose(); await s.handle.dispose();
});

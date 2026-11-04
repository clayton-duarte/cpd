import { describe, expect, it } from "vitest";
import type { WebError } from "../../protocol/domain.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import { createNormalizer } from "./jsonl.ts";
import type { WatchBoundary, WatchEvent } from "./watch.ts";
import { watchSession } from "./watch.ts";
import type { FileBoundary } from "./window.ts";

const enc = new TextEncoder();
const PATH = "/s/session.jsonl";
const userLine = (i: number): string =>
  JSON.stringify({ type: "message", id: `m${i}`, message: { role: "user", content: `message ${i}` } });

class FakeFile implements FileBoundary {
  bytes: Uint8Array;
  identity = "1:1";
  reads: Array<[number, number]> = [];
  failStat = false;
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
    if (this.failStat) return Promise.reject(new Error("EIO"));
    return Promise.resolve({ size: this.bytes.length, identity: this.identity });
  }
  async write(): Promise<void> {}
  read(_p: string, start: number, end: number): Promise<Uint8Array> {
    this.reads.push([start, end]);
    return Promise.resolve(this.bytes.slice(start, end));
  }
}

class FakeWatch implements WatchBoundary {
  handlers: Array<() => void> = [];
  disposed = 0;
  paths: string[] = [];
  watch(path: string, onChange: () => void): Disposable {
    this.paths.push(path);
    this.handlers.push(onChange);
    return {
      dispose: () => {
        this.disposed += 1;
        this.handlers = this.handlers.filter((h) => h !== onChange);
      },
    };
  }
  fire(): void {
    for (const h of [...this.handlers]) h();
  }
}

const flush = async (): Promise<void> => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};

function setup(initial: string) {
  const fs = new FakeFile(initial);
  const watch = new FakeWatch();
  const events: WatchEvent[] = [];
  const errors: WebError[] = [];
  const offset = fs.bytes.length;
  const handle = watchSession({
    path: PATH,
    fs,
    watch,
    normalizer: createNormalizer(),
    offset,
    identity: "1:1",
    onEvent: (e) => events.push(e),
    onError: (e) => errors.push(e),
  });
  return { fs, watch, events, errors, handle, offset };
}

describe("pi watch", () => {
  it("a file change reads only the appended bytes", async () => {
    const { fs, watch, offset } = setup(`${userLine(0)}\n${userLine(1)}\n`);
    expect(watch.paths).toEqual([PATH]);
    const added = `${userLine(2)}\n`;
    fs.append(added);
    watch.fire();
    await flush();
    expect(fs.reads).toEqual([[offset, offset + enc.encode(added).length]]);
  });

  it("an append produces update items for the new lines", async () => {
    const { fs, watch, events, offset } = setup(`${userLine(0)}\n`);
    const call = JSON.stringify({ type: "message", id: "a1", message: { role: "assistant", content: [{ type: "toolCall", id: "c1", name: "read", arguments: {} }] } });
    const result = JSON.stringify({ type: "message", id: "r1", message: { role: "toolResult", toolCallId: "c1", toolName: "read", content: [{ type: "text", text: "ok" }], isError: false } });
    const partial = userLine(9).slice(0, 15);
    fs.append(`${userLine(1)}\n${call}\n${partial}`);
    watch.fire();
    await flush();
    expect(events).toHaveLength(1);
    const e = events[0] as Extract<WatchEvent, { kind: "append" }>;
    expect(e.kind).toBe("append");
    expect(e.result.added.map((i) => i.id)).toEqual(["u:m1", "tool:c1"]);
    expect(e.endOffset).toBe(offset + enc.encode(`${userLine(1)}\n${call}\n`).length);
    const rest = userLine(9).slice(15);
    fs.append(`${rest}\n${result}\n`);
    watch.fire();
    await flush();
    const second = events[1] as Extract<WatchEvent, { kind: "append" }>;
    expect(second.result.added.map((i) => i.id)).toEqual(["u:m9"]);
    expect(second.result.updated.map((i) => i.id)).toEqual(["tool:c1"]);
    expect(second.endOffset).toBe(fs.bytes.length);
  });

  it("truncation or replacement resets the window", async () => {
    const truncated = setup(`${userLine(0)}\n${userLine(1)}\n`);
    truncated.fs.bytes = enc.encode(`${userLine(0)}\n`);
    truncated.watch.fire();
    await flush();
    expect(truncated.events).toEqual([{ kind: "reset", reason: "truncated" }]);
    truncated.fs.append(`${userLine(5)}\n`);
    truncated.watch.fire();
    await flush();
    expect(truncated.events).toHaveLength(1);

    const replaced = setup(`${userLine(0)}\n`);
    replaced.fs.identity = "1:2";
    replaced.fs.append(`${userLine(1)}\n`);
    replaced.watch.fire();
    await flush();
    expect(replaced.events).toEqual([{ kind: "reset", reason: "replaced" }]);
  });

  it("stopping the watcher releases the watch handle", async () => {
    const { fs, watch, events, handle } = setup(`${userLine(0)}\n`);
    fs.append(`${userLine(1)}\n`);
    watch.fire();
    handle.dispose();
    handle.dispose();
    await flush();
    expect(watch.disposed).toBe(1);
    expect(watch.handlers).toHaveLength(0);
    expect(events).toEqual([]);
    fs.append(`${userLine(2)}\n`);
    watch.fire();
    await flush();
    expect(events).toEqual([]);
  });
});

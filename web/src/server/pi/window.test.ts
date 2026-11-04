import { describe, expect, it, vi } from "vitest";
import type { UserItem } from "../../protocol/domain.ts";
import { CHUNK_BYTES, cursorOf, offsetOf, readEarlier, readTail } from "./window.ts";
import type { FileBoundary } from "./window.ts";

const enc = new TextEncoder();
const PATH = "/s/session.jsonl";

class FakeFile implements FileBoundary {
  bytes: Uint8Array;
  identity = "1:1";
  reads: Array<[number, number]> = [];
  constructor(text: string) {
    this.bytes = enc.encode(text);
  }
  stat(): Promise<{ size: number; identity: string }> {
    return Promise.resolve({ size: this.bytes.length, identity: this.identity });
  }
  read(_path: string, start: number, end: number): Promise<Uint8Array> {
    this.reads.push([start, end]);
    return Promise.resolve(this.bytes.slice(start, end));
  }
  get readBytes(): number {
    return this.reads.reduce((n, [s, e]) => n + (e - s), 0);
  }
}

const userLine = (i: number, pad = ""): string =>
  JSON.stringify({ type: "message", id: `m${i}`, message: { role: "user", content: `message ${i} é✓ ${pad}` } });

function build(count: number, pad = ""): { text: string; starts: number[] } {
  const starts: number[] = [];
  let offset = 0;
  const chunks: string[] = [];
  for (let i = 0; i < count; i++) {
    starts.push(offset);
    const line = `${userLine(i, pad)}\n`;
    chunks.push(line);
    offset += enc.encode(line).length;
  }
  return { text: chunks.join(""), starts };
}
const ids = (w: { items: readonly { id: string }[] }): string[] => w.items.map((i) => i.id);

describe("pi window reads", () => {
  it("build encodes each line once, so its cost is linear", () => {
    const spy = vi.spyOn(TextEncoder.prototype, "encode");
    try {
      const { text } = build(3000, "x".repeat(60));
      const encoded = spy.mock.calls.reduce((n, [arg]) => n + (typeof arg === "string" ? arg.length : 0), 0);
      expect(encoded).toBeLessThanOrEqual(2 * text.length);
    } finally {
      spy.mockRestore();
    }
  });

  it("tail window returns the last N items without reading the whole file", async () => {
    const { text, starts } = build(3000, "x".repeat(60));
    const fs = new FakeFile(text);
    const tail = await readTail(PATH, fs, 40);
    expect(ids(tail.window)).toEqual(Array.from({ length: 40 }, (_, i) => `u:m${2960 + i}`));
    expect(tail.window.hasEarlier).toBe(true);
    expect(tail.window.cursor).toBe(cursorOf(starts[2960] as number));
    expect(tail.startOffset).toBe(starts[2960]);
    expect(tail.endOffset).toBe(fs.bytes.length);
    expect(tail.identity).toBe("1:1");
    expect(fs.reads.length).toBeLessThanOrEqual(3);
    for (const [s, e] of fs.reads) expect(e - s).toBeLessThanOrEqual(CHUNK_BYTES);
    expect(fs.readBytes).toBeLessThan(fs.bytes.length / 10);
    expect(offsetOf(cursorOf(123))).toBe(123);
    expect(() => offsetOf("dur:3")).toThrowError(expect.objectContaining({ code: "invalid_message" }));
    expect(() => offsetOf("pi:-1")).toThrowError(expect.objectContaining({ code: "invalid_message" }));
  });

  it("loadEarlier pages backwards by byte offset", async () => {
    const { text, starts } = build(100);
    const fs = new FakeFile(text);
    const tail = await readTail(PATH, fs, 40);
    expect(ids(tail.window)[0]).toBe("u:m60");
    const page = await readEarlier(PATH, fs, tail.window.cursor as string, 40);
    expect(ids(page.window)).toEqual(Array.from({ length: 40 }, (_, i) => `u:m${20 + i}`));
    expect(page.startOffset).toBe(starts[20]);
    expect(page.window.cursor).toBe(cursorOf(starts[20] as number));
    const last = await readEarlier(PATH, fs, page.window.cursor as string, 40);
    expect(ids(last.window)).toEqual(Array.from({ length: 20 }, (_, i) => `u:m${i}`));
    expect(last.window.hasEarlier).toBe(false);
    expect(last.window.cursor).toBeNull();
    expect(last.startOffset).toBe(0);
  });

  it("windows never split a line", async () => {
    const { text } = build(1200, "é✓".repeat(20));
    const fs = new FakeFile(text);
    expect(fs.bytes.length).toBeGreaterThan(CHUNK_BYTES * 2);
    const tail = await readTail(PATH, fs, 5000);
    expect(tail.window.items).toHaveLength(1200);
    expect(tail.window.hasEarlier).toBe(false);
    tail.window.items.forEach((item, i) => {
      expect((item as UserItem).text).toBe(`message ${i} é✓ ${"é✓".repeat(20)}`);
    });
    for (const [s, e] of fs.reads) expect(e - s).toBeLessThanOrEqual(CHUNK_BYTES);
    const small = new FakeFile(text);
    const t = await readTail(PATH, small, 40);
    const p = await readEarlier(PATH, small, t.window.cursor as string, 40);
    expect(p.window.items).toHaveLength(40);
    expect((p.window.items[0] as UserItem).text).toContain("é✓");
    expect(new Set([...ids(t.window), ...ids(p.window)]).size).toBe(80);
  });

  it("a partial last line is not emitted until complete", async () => {
    const { text } = build(5);
    const partial = userLine(5).slice(0, 20);
    const fs = new FakeFile(text + partial);
    const first = await readTail(PATH, fs, 40);
    expect(ids(first.window)).toEqual(["u:m0", "u:m1", "u:m2", "u:m3", "u:m4"]);
    expect(first.endOffset).toBe(enc.encode(text).length);
    fs.bytes = enc.encode(`${text}${userLine(5)}\n`);
    const second = await readTail(PATH, fs, 40);
    expect(ids(second.window)).toHaveLength(6);
    expect(second.endOffset).toBe(fs.bytes.length);
  });

  it("an empty file yields an empty window", async () => {
    const empty = await readTail(PATH, new FakeFile(""), 40);
    expect(empty.window).toEqual({ items: [], hasEarlier: false, cursor: null });
    expect([empty.startOffset, empty.endOffset]).toEqual([0, 0]);
    expect(empty.openDialogs).toEqual([]);
    const noNewline = await readTail(PATH, new FakeFile(userLine(0)), 40);
    expect(noNewline.window).toEqual({ items: [], hasEarlier: false, cursor: null });
    expect([noNewline.startOffset, noNewline.endOffset]).toEqual([0, 0]);
  });
});

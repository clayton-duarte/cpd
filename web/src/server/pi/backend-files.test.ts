import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { describe, expect, it } from "vitest";
import type { SessionRef, WireFile } from "../../protocol/domain.ts";
import { PI_SEND_TEXT_MAX } from "../../protocol/domain.ts";
import type { Disposable, GitBoundary } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { createPiBackend } from "./backend.ts";
import type { Cpd } from "../cockpit/cli.ts";
import type { SessionHandle } from "../hub/connection.ts";
import type { WatchBoundary } from "./watch.ts";
import type { FileBoundary } from "./window.ts";

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
  return { fs, cpd, out, handle, backend };
}

const send = (c: Array<readonly string[]>) => c.filter((x) => x[1] === "send");

it("a 35 KB text attachment sends and the message holds its path, not its content", async () => {
  const s = await setup({ text: "", now: 42 });
  const content = "A".repeat(35 * 1024);
  const textFile: WireFile = { name: "large.txt", text: content };

  await (s.handle as unknown as { submit: (input: { clientSubmissionId: string; text: string; images: never[]; files?: WireFile[]; whenBusy: string }) => Promise<void> }).submit({
    clientSubmissionId: "c1",
    text: "please review",
    images: [],
    files: [textFile],
    whenBusy: "followUp",
  });

  const written = s.fs.writes.find((w) => w.path.endsWith("-0-large.txt"));
  expect(written?.path).toMatch(/^\/att\/s1-42-[0-9a-f-]{36}-0-large\.txt$/);
  expect(written).toBeDefined();
  expect(new TextDecoder().decode(written?.bytes)).toBe(content);

  const calls = send(s.cpd.calls);
  expect(calls).toHaveLength(1);
  const sentText = calls[0]?.at(-1);
  expect(sentText).toBeDefined();
  expect(sentText?.length).toBeLessThan(PI_SEND_TEXT_MAX);
  expect(sentText).toContain(`[Attached file: ${written!.path}]`);
  expect(sentText).not.toContain(content);
});


it("concurrent submissions in one millisecond use disjoint attachment paths", async () => {
  const s = await setup({ text: "", now: 42 });
  const file = (text: string) => [{ name: "context.txt", text }];
  const received: string[] = [];
  const json = s.cpd.json.bind(s.cpd);
  s.cpd.json = (args) => {
    if (args[1] === "send") {
      const path = args.at(-1)!.match(/\[Attached file: (.+)\]/)![1];
      const written = s.fs.writes.findLast((w) => w.path === path)!;
      received.push(new TextDecoder().decode(written.bytes));
    }
    return json(args);
  };
  await Promise.all([
    s.handle.submit({ clientSubmissionId: "c1", text: "first", images: [], files: file("first content"), whenBusy: "followUp" }),
    s.handle.submit({ clientSubmissionId: "c2", text: "second", images: [], files: file("second content"), whenBusy: "followUp" }),
  ]);
  expect(received).toEqual(["first content", "second content"]);
  expect(s.fs.writes).toHaveLength(2);
  expect(new Set(s.fs.writes.map((w) => w.path)).size).toBe(2);
  const paths = send(s.cpd.calls).map((args) => args.at(-1)!.match(/\[Attached file: (.+)\]/)![1]);
  expect(paths[0]).not.toBe(paths[1]);
});


it("different handles with identical client IDs still get independent attachment paths", async () => {
  const s = await setup({ text: "", now: 42 });
  const other = await s.backend.attach({ ref: ref(), file: FILE, tierOptions: ["S3"] }, () => {});
  const input = { clientSubmissionId: "c1", text: "context", images: [], files: [{ name: "f.txt", text: "bytes" }], whenBusy: "followUp" as const };
  await Promise.all([s.handle.submit(input), other.submit(input)]);
  expect(new Set(s.fs.writes.map((w) => w.path)).size).toBe(2);
  await s.handle.dispose();
  await other.dispose();
});


it.each(["x".repeat(246) + ".txt", "é".repeat(124) + ".txt", "x".repeat(250), "x." + "t".repeat(248)])("stores a valid long picked filename within the component byte limit: %s", async (name) => {
  const dir = await mkdtemp(join(tmpdir(), "cpd-qa-file-name-"));
  const s = await setup({ text: "" });
  try {
    await writeFile(join(dir, name), "picked content");
    expect(await readFile(join(dir, name), "utf8")).toBe("picked content");
    s.fs.write = async (path, bytes) => {
      await writeFile(join(dir, basename(path)), bytes);
      s.fs.writes.push({ path, bytes });
    };
    await s.handle.submit({ clientSubmissionId: "long-name", text: "review", images: [], files: [{ name, text: "picked content" }], whenBusy: "followUp" });
    expect(send(s.cpd.calls)).toHaveLength(1);
    const written = s.fs.writes[0]!;
    expect(Buffer.byteLength(basename(written.path))).toBeLessThanOrEqual(255);
    expect(basename(written.path)).toMatch(/^s1-1000-[0-9a-f-]{36}-0-/);
    if (name.endsWith(".txt")) expect(written.path.endsWith(".txt")).toBe(true);
    expect(await readFile(join(dir, basename(written.path)), "utf8")).toBe("picked content");
    expect(send(s.cpd.calls)[0]?.at(-1)).toContain(`[Attached file: ${written.path}]`);
  } finally {
    await s.handle.dispose();
    await rm(dir, { recursive: true, force: true });
  }
});

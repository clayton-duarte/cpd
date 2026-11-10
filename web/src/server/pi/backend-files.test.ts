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
  return { fs, cpd, out, handle };
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

  expect(s.fs.writes.map((w) => w.path)).toContain("/att/s1-42-0-large.txt");
  const written = s.fs.writes.find((w) => w.path === "/att/s1-42-0-large.txt");
  expect(written).toBeDefined();
  expect(new TextDecoder().decode(written?.bytes)).toBe(content);

  const calls = send(s.cpd.calls);
  expect(calls).toHaveLength(1);
  const sentText = calls[0]?.at(-1);
  expect(sentText).toBeDefined();
  expect(sentText?.length).toBeLessThan(PI_SEND_TEXT_MAX);
  expect(sentText).toContain("[Attached file: /att/s1-42-0-large.txt]");
  expect(sentText).not.toContain(content);
});

import { describe, expect, it } from "vitest";
import type { SessionRef } from "../../protocol/domain.ts";
import { PI_SEND_TEXT_MAX, IMAGE_MAX_COUNT, IMAGE_MAX_TOTAL_BASE64 } from "../../protocol/domain.ts";
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
const rejects = async (p: Promise<unknown>) => {
  try {
    await p;
  } catch (e) {
    return e as { code?: string; message: string };
  }
  throw new Error("did not reject");
};
const send = (c: Array<readonly string[]>) => c.filter((x) => x[1] === "send");
const img = (mimeType: "image/png" | "image/jpeg" | "image/gif" | "image/webp", dataBase64: string) => ({ mimeType, dataBase64 });
const submit = (s: { handle: SessionHandle }, text: string, images: ReturnType<typeof img>[]) =>
  s.handle.submit({ clientSubmissionId: "c1", text, images, whenBusy: "followUp" });

describe("pi backend images", () => {
  it("the pi backend declares image support", async () => {
    const s = await setup({ text: "" });
    expect(s.handle.capabilities.images).toEqual({ supported: true, maxCount: IMAGE_MAX_COUNT, maxTotalBase64: IMAGE_MAX_TOTAL_BASE64 });
  });

  it("submit writes each image under the attachments directory and appends its path to the text", async () => {
    const s = await setup({ text: "", now: 55 });
    await submit(s, "look", [img("image/png", "AQID"), img("image/jpeg", "BAUG")]);
    expect(s.fs.writes.map((w) => [w.path, [...w.bytes]])).toEqual([
      ["/att/s1-55-0.png", [1, 2, 3]],
      ["/att/s1-55-1.jpg", [4, 5, 6]],
    ]);
    const text = "look\n\n[Attached image: /att/s1-55-0.png]\n[Attached image: /att/s1-55-1.jpg]";
    expect(send(s.cpd.calls)).toEqual([["session", "send", "s1", "--origin", "webui", "--unblocks", "User message from the web chat", "--", text]]);
    const none = await setup({ text: "" });
    await submit(none, "plain", []);
    expect(none.fs.writes).toEqual([]);
    expect(send(none.cpd.calls)[0]?.at(-1)).toBe("plain");
  });

  it("submit names the files with the session id, the timestamp, the index and the mime extension", async () => {
    const s = await setup({ text: "", attachmentsDir: "/tmp/a", now: 9 });
    await submit(s, "x", [img("image/png", "AQID"), img("image/jpeg", "AQID"), img("image/gif", "AQID"), img("image/webp", "AQID")]);
    expect(s.fs.writes.map((w) => w.path)).toEqual(["/tmp/a/s1-9-0.png", "/tmp/a/s1-9-1.jpg", "/tmp/a/s1-9-2.gif", "/tmp/a/s1-9-3.webp"]);
  });

  it("a text too long once the paths are appended is refused invalid_submission", async () => {
    const s = await setup({ text: "" });
    const err = await rejects(submit(s, "x".repeat(PI_SEND_TEXT_MAX - 5), [img("image/png", "AQID")]));
    expect(err.code).toBe("invalid_submission");
    expect(send(s.cpd.calls)).toEqual([]);
  });
});

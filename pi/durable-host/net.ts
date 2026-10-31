import { chmodSync } from "node:fs";
import { mkdir, chmod, stat, unlink } from "node:fs/promises";
import { connect, createServer } from "node:net";
import { dirname } from "node:path";
import { CpdError } from "../lib/cpd.ts";
import type { Connection } from "./server.ts";
import { dirVerdict, LineSplitter, staleVerdict, type Probe } from "./sockets.ts";

type Accept = (conn: Connection) => { receive(line: string): void; closed(): void };

const probe = (path: string): Promise<Probe> =>
  new Promise((resolve) => {
    const s = connect(path);
    s.once("connect", () => (s.destroy(), resolve("connected")));
    s.once("error", (e: NodeJS.ErrnoException) => resolve(e.code === "ENOENT" ? "absent" : e.code === "ECONNREFUSED" ? "refused" : { error: String(e.code ?? e.message) }));
  });

/** The real unix socket behind `Deps.listen`: no rule of its own (the rules are in sockets.ts). */
export async function listenOn(path: string, accept: Accept): Promise<{ close(): Promise<void> }> {
  const dir = dirname(path);
  const info = await stat(dir).then((s) => ({ exists: true as const, isDir: s.isDirectory(), uid: s.uid, mode: s.mode }), () => ({ exists: false as const }));
  const verdict = dirVerdict(info, process.getuid!());
  if (verdict === "refuse") throw new CpdError("usage", `${dir} is not a directory owned by this user`);
  if (verdict === "create") await mkdir(dir, { recursive: true, mode: 0o700 });
  if (verdict === "chmod" || verdict === "create") await chmod(dir, 0o700);
  const stale = staleVerdict(await probe(path));
  if (stale === "host_running") throw new Error("host_running");
  if (stale === "fail") throw new Error(`cannot probe ${path}`);
  if (stale === "remove") await unlink(path);
  const server = createServer((socket) => {
    const splitter = new LineSplitter();
    const h = accept({ write: (l) => void socket.write(l), buffered: () => socket.writableLength, close: () => void socket.end() });
    socket.on("data", (chunk) => {
      const { lines, overLimit } = splitter.push(chunk);
      for (const l of lines) h.receive(l);
      if (overLimit !== undefined) h.receive(overLimit);
    });
    socket.on("close", () => h.closed());
    socket.on("error", () => socket.destroy());
  });
  const old = process.umask(0o177);
  try {
    await new Promise<void>((resolve, reject) => server.once("error", reject).listen(path, resolve));
  } finally {
    process.umask(old);
  }
  chmodSync(path, 0o600);
  return { close: () => new Promise((resolve) => server.close(() => resolve())) };
}

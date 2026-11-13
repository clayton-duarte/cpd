import { createServer, type Socket } from "node:net";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";
import { realBoundaries } from "./plugin.ts";

it("terminal socket teardown releases the local socket when a peer holds its half open", async () => {
  const dir = await mkdtemp(join(tmpdir(), "cpd-socket-close-"));
  let peer: Socket | undefined;
  const server = createServer({ allowHalfOpen: true }, (socket) => { peer = socket; socket.resume(); });
  await new Promise<void>((resolve, reject) => { server.once("error", reject); server.listen(join(dir, "host.sock"), resolve); });
  const socket = await realBoundaries({}).socketFactory.connect(join(dir, "host.sock"));
  let timer: ReturnType<typeof setTimeout> | undefined;
  let subscription: { dispose(): void } | undefined;
  try {
    const closed = new Promise<boolean>((resolve) => {
      subscription = socket.onClose(() => resolve(true));
      timer = setTimeout(() => resolve(false), 1000);
    });
    socket.destroy();
    expect(await closed).toBe(true);
  } finally {
    clearTimeout(timer);
    subscription?.dispose();
    peer?.destroy();
    socket.destroy();
    await new Promise<void>((resolve) => server.close(() => resolve()));
    await rm(dir, { recursive: true, force: true });
  }
});

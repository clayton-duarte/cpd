import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { expect, it, vi } from "vitest";
import { prepareHandoff } from "./b101-handoff.ts";
import { createDedupStore, fileDedupStorage } from "./hub/dedup.ts";

it("late receipt is replayed after safe handoff", async () => {
  const dir = mkdtempSync(join(process.cwd(), ".handoff-test-"));
  try {
    const file = join(dir, "receipts.json");
    const disk = fileDedupStorage(file);
    const read = vi.fn(() => disk.read());
    const write = vi.fn((value: string) => disk.write(value));
    const storage = { read, write };
    const old = createDedupStore({ storage, cap: 1000 });
    const candidate = prepareHandoff(storage);
    // Preparing must not read or modify production receipts.
    expect(read).toHaveBeenCalledTimes(1);
    expect(write).not.toHaveBeenCalled();

    const execute = vi.fn(async () => ({ actionId: "request-1", output: { revision: 42 } }));
    const result = await old.run("action:request-1", execute);
    const persisted = readFileSync(file, "utf8");
    expect(execute).toHaveBeenCalledTimes(1);
    expect(persisted).toContain("request-1");

    let disposed = false;
    const drainAndDispose = vi.fn(async () => { disposed = true; });
    read.mockImplementation(() => {
      expect(disposed).toBe(true);
      return disk.read();
    });
    const replacement = await candidate.transfer(drainAndDispose);
    expect(drainAndDispose).toHaveBeenCalledOnce();
    expect(read).toHaveBeenCalledTimes(2);
    const writesBeforeReplay = write.mock.calls.length;
    const replay = await replacement.run("action:request-1", execute);
    expect(replay).toEqual(result);
    expect(execute).toHaveBeenCalledTimes(1);
    expect(write).toHaveBeenCalledTimes(writesBeforeReplay);
    expect(readFileSync(file, "utf8")).toBe(persisted);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

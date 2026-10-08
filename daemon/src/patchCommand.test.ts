import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, afterEach } from "vitest";
import { addPlanJob, openEngine, patchPlanJob, type Engine } from "./engine.ts";

// Direct engine-level test for the `command` clear semantics added for H12: `patchPlanJob` must
// treat an explicit `null` as "clear" and an absent key as "leave untouched". The `?? job.x` idiom
// used for title/needs cannot express this (`null ?? job.command` yields the old value), so this
// guards the explicit `'command' in patch` check.

describe("patchPlanJob command semantics", () => {
  let dir: string;
  let engine: Engine | undefined;

  afterEach(async () => {
    await engine?.close();
    engine = undefined;
    if (dir) rmSync(dir, { recursive: true, force: true });
  });

  it("sets, clears with explicit null, and leaves untouched when the key is absent", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-patch-command-"));
    engine = await openEngine({ dbPath: join(dir, "test.sqlite") });
    const conversationId = engine.root.id as unknown as number;

    const job = await addPlanJob(engine, conversationId, { title: "Draft" });
    expect(job.command).toBeNull();

    const withCommand = await patchPlanJob(engine, conversationId, { id: job.id, command: "echo hi" });
    expect(withCommand.command).toBe("echo hi");

    const untouched = await patchPlanJob(engine, conversationId, { id: job.id, title: "Renamed" });
    expect(untouched.command).toBe("echo hi");
    expect(untouched.title).toBe("Renamed");

    const cleared = await patchPlanJob(engine, conversationId, { id: job.id, command: null });
    expect(cleared.command).toBeNull();
  });
});

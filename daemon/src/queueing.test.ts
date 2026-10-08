import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, afterEach } from "vitest";
import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import { Harness, createRegistry, InboxDoc, type Conversation } from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { createModels } from "@earendil-works/pi-ai/models";
import { fauxProvider, fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { submitPrompt, getQueueDepth, type Engine } from "./engine.ts";

/** L2: pi-durable's documented defaults (`DEFAULT_RETRY_POLICY` / `DEFAULT_COMPACTION_POLICY`),
 * restated explicitly -- matches `HARNESS_SETTINGS` in engine.ts (D-whatever this card lands as). */
const HARNESS_SETTINGS = {
  retry: { enabled: true, maxRetries: 3, baseDelayMs: 2000 },
  compaction: { enabled: true, reserveTokens: 16384, keepRecentTokens: 20000, backgroundTokens: 32768 },
};

/** Opens a harness with a faux provider and, optionally, the given settings -- never a real
 * model, matching the verified recipe from durability.integration.test.ts. */
async function openTestHarness(
  dbPath: string,
  options: { settings?: typeof HARNESS_SETTINGS } = {},
): Promise<{ harness: Harness; root: Conversation; faux: ReturnType<typeof fauxProvider> }> {
  const storage = await openNodeSqliteStorage(dbPath);
  const faux = fauxProvider();
  const models = createModels();
  models.setProvider(faux.provider);
  const harness = await Harness.open(
    storage,
    {
      models,
      registry: createRegistry(),
      settings: options.settings,
    },
    ctx,
  );
  harness.resume();
  const root = await harness.root(ctx, {
    agent: { model: { provider: "faux", modelId: "faux-1" }, instructions: "test" },
  });
  return { harness, root, faux };
}

describe("L2: retry, compaction and queueing settings", () => {
  let dir: string;

  afterEach(() => {
    if (dir && existsSync(dir)) rmSync(dir, { recursive: true, force: true });
  });

  // --- Sabotage C: retry disabled in settings ------------------------------------------------
  // With the real `HARNESS_SETTINGS` (retry.enabled: true), a generation failure is retried
  // instead of becoming a permanent `unanswered`. This test fixture proves the DIFFERENCE: with
  // retry disabled, a single faux error settles as unanswered immediately.
  it("retry.enabled: true lets a single transient faux error still reach done (sabotage C disables this)", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-retry-test-"));
    const { harness, root, faux } = await openTestHarness(join(dir, "test.sqlite"), {
      settings: HARNESS_SETTINGS,
    });

    // First attempt errors (transient-looking message), second attempt succeeds. Only possible
    // to observe "done" if the engine actually retries.
    faux.setResponses([
      () => {
        throw new Error("503 Service Unavailable");
      },
      fauxAssistantMessage("recovered"),
    ]);

    const result = await submitPrompt(root, "hello");
    expect(result.status).toBe("done");

    await harness.close(ctx);
  });

  it("SABOTAGE C: retry disabled means the same transient error settles unanswered, not done", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-retry-sabotage-test-"));
    const sabotaged = {
      ...HARNESS_SETTINGS,
      retry: { ...HARNESS_SETTINGS.retry, enabled: false },
    };
    const { harness, root, faux } = await openTestHarness(join(dir, "test.sqlite"), {
      settings: sabotaged,
    });

    faux.setResponses([
      () => {
        throw new Error("503 Service Unavailable");
      },
      fauxAssistantMessage("recovered"),
    ]);

    const result = await submitPrompt(root, "hello");
    expect(result.status).toBe("unanswered");

    await harness.close(ctx);
  });

  // --- whenBusy / ordering (A, B) -------------------------------------------------------------
  // `submitPrompt` always passes `whenBusy: "followUp"`. Two prompts submitted back-to-back on a
  // busy conversation must BOTH be answered, in submission order.
  it("two prompts submitted back-to-back are both answered, in submission order (whenBusy: followUp)", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-queue-order-test-"));
    const { harness, root, faux } = await openTestHarness(join(dir, "test.sqlite"), {
      settings: HARNESS_SETTINGS,
    });

    faux.setResponses([
      async () => {
        await new Promise((resolve) => setTimeout(resolve, 300));
        return fauxAssistantMessage("first reply");
      },
      fauxAssistantMessage("second reply"),
    ]);

    const firstSubmit = submitPrompt(root, "first prompt");
    // Give the first submission a beat to actually start the run before the second arrives, so
    // this is a genuine "conversation busy" submission, not two simultaneously-idle ones.
    await new Promise((resolve) => setTimeout(resolve, 50));
    const secondSubmit = submitPrompt(root, "second prompt");

    const [first, second] = await Promise.all([firstSubmit, secondSubmit]);
    expect(first.status).toBe("done");
    expect(second.status).toBe("done");

    const view = await root.context(ctx, {});
    const assistantTexts = view.entries
      .flatMap((entry) => entry.model ?? [])
      .filter((message) => message.role === "assistant")
      .map((message) => (typeof message.content === "string" ? message.content : message.content.map((p: { type: string; text?: string }) => (p.type === "text" ? p.text : "")).join("")));
    expect(assistantTexts).toEqual(["first reply", "second reply"]);

    await harness.close(ctx);
  });

  it("SABOTAGE B: whenBusy reject throws ConversationBusy for the second prompt instead of queueing it", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-queue-reject-test-"));
    const { harness, root, faux } = await openTestHarness(join(dir, "test.sqlite"), {
      settings: HARNESS_SETTINGS,
    });
    faux.setResponses([
      async () => {
        await new Promise((resolve) => setTimeout(resolve, 300));
        return fauxAssistantMessage("first reply");
      },
      fauxAssistantMessage("second reply"),
    ]);

    const firstSubmit = root.submit({ type: "input", content: "first prompt", whenBusy: "followUp" }, ctx);
    await new Promise((resolve) => setTimeout(resolve, 50));

    await expect(root.submit({ type: "input", content: "second prompt", whenBusy: "reject" }, ctx)).rejects.toThrow();

    await (await firstSubmit).wait(ctx);
    await harness.close(ctx);
  });

  // --- Queue depth (D) ------------------------------------------------------------------------
  it("getQueueDepth reads the live count from pi.inbox while a follow-up is queued", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-queue-depth-test-"));
    const { harness, root, faux } = await openTestHarness(join(dir, "test.sqlite"), {
      settings: HARNESS_SETTINGS,
    });
    const engine = { harness } as unknown as Engine;

    expect(await getQueueDepth(engine, root.id)).toBe(0);

    faux.setResponses([
      async () => {
        await new Promise((resolve) => setTimeout(resolve, 300));
        return fauxAssistantMessage("first reply");
      },
      fauxAssistantMessage("second reply"),
    ]);

    const firstSubmit = submitPrompt(root, "first prompt");
    await new Promise((resolve) => setTimeout(resolve, 50));
    const secondSubmit = submitPrompt(root, "second prompt");
    // The second submission is queued (conversation busy with the first run) before it settles.
    await new Promise((resolve) => setTimeout(resolve, 50));
    const depthWhileQueued = await getQueueDepth(engine, root.id);
    expect(depthWhileQueued).toBe(1);

    await Promise.all([firstSubmit, secondSubmit]);
    expect(await getQueueDepth(engine, root.id)).toBe(0);

    await harness.close(ctx);
  });

  it("SABOTAGE D: a hardcoded 0 would read InboxDoc directly and diverge once something is actually queued", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-queue-depth-sabotage-test-"));
    const { harness, root } = await openTestHarness(join(dir, "test.sqlite"), { settings: HARNESS_SETTINGS });
    const engine = { harness } as unknown as Engine;

    // Directly write an inbox item (bypassing submit's run machinery) to prove getQueueDepth
    // reflects the real document, not a hardcoded 0.
    await harness.commit(async (tx) => {
      const draft = await tx.doc(InboxDoc, root.id);
      draft.items.push({ id: "fake-submission-id" as never, mode: "followUp", content: { type: "input", content: "queued" } as never });
    }, ctx);

    const depth = await getQueueDepth(engine, root.id);
    expect(depth).toBe(1);

    await harness.close(ctx);
  });
});

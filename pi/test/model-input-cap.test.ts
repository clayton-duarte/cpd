/**
 * Regression tests for hard model input cap (pro-d559, contract §6.23, docs/architecture/model-input-cap.md).
 *
 * Requirements:
 * (1) cap values: 1M -> 200000, 256K -> 200000, 200K -> 160000, 128K -> 102400 (and boundaries);
 * (2) boundary: input equal to cap is sent, cap + 1 compacts;
 * (3) prompt and tool-schema overhead is counted;
 * (4) oversized incoming message or tool result is handled;
 * (5) model downswitch (1M to 128K) recalculates cap before next request;
 * (6) when rebuilt input is still too large, model is not dispatched and explicit error is raised;
 * (7) stored transcript is unchanged;
 * (8) invalid or missing window metadata fails loud.
 */

import assert from "node:assert/strict";
import test from "node:test";
import {
  calculateModelInputCap,
  guardModelDispatch,
  ContextWindowInvalidError,
  InputOverCapError,
  type DispatchRequest,
} from "../lib/model-input-cap.ts";

test("cap calculation matches pro-d559 for 1M, 256K, 200K, 128K windows", () => {
  // Required scenarios from pro-d559 & spec
  assert.equal(calculateModelInputCap(1_000_000), 200_000);
  assert.equal(calculateModelInputCap(256_000), 200_000);
  assert.equal(calculateModelInputCap(200_000), 160_000);
  assert.equal(calculateModelInputCap(128_000), 102_400);

  // Boundary checks from docs/architecture/model-input-cap.md §1
  assert.equal(calculateModelInputCap(250_000), 200_000);
  assert.equal(calculateModelInputCap(250_001), 200_000);
  assert.equal(calculateModelInputCap(8_192), 6_553); // Integer rounding floor(8192 * 4 / 5) = 6553, not 6554
  assert.equal(calculateModelInputCap(1), 0);
});

test("boundary: input equal to cap is sent, cap + 1 compacts", async () => {
  const model = { provider: "anthropic", id: "claude-3-opus", contextWindow: 200_000 };
  const cap = 160_000;

  // Case A: input equal to cap (160,000) is sent without compaction
  let compactCallsA = 0;
  let sentCallsA = 0;
  await guardModelDispatch(
    { model, totalTokens: cap, messages: [{ role: "user", content: "hello" }] },
    {
      compact: () => {
        compactCallsA++;
        return { compactedMessages: [], compactedTokens: 100_000 };
      },
      send: () => {
        sentCallsA++;
        return { ok: true };
      },
    }
  );
  assert.equal(compactCallsA, 0, "Input equal to cap must not trigger compaction");
  assert.equal(sentCallsA, 1, "Input equal to cap must be dispatched");

  // Case B: cap + 1 triggers compaction, then dispatches with compacted input
  let compactCallsB = 0;
  let sentCallsB = 0;
  let dispatchedPayload: DispatchRequest | undefined;
  await guardModelDispatch(
    { model, totalTokens: cap + 1, messages: [{ role: "user", content: "boundary" }] },
    {
      compact: (msgs) => {
        compactCallsB++;
        return { compactedMessages: [{ role: "user", content: "compacted" }], compactedTokens: 120_000 };
      },
      send: (req) => {
        sentCallsB++;
        dispatchedPayload = req;
        return { ok: true };
      },
    }
  );
  assert.equal(compactCallsB, 1, "Input equal to cap + 1 must trigger compaction");
  assert.equal(sentCallsB, 1, "Compacted input fitting within cap must be dispatched");
  assert.equal(dispatchedPayload?.totalTokens, 120_000);
});

test("prompt and tool-schema overhead is counted in effective input", async () => {
  // Cap for 200K is 160,000.
  // Message tokens alone = 150,000 (which is <= 160,000).
  // But system prompt (8,000) + tool schemas (3,000) push effective input to 161,000 (> 160,000).
  const model = { provider: "anthropic", id: "claude-3-sonnet", contextWindow: 200_000 };
  let compacted = false;
  let sent = false;

  await guardModelDispatch(
    {
      model,
      breakdown: {
        systemPromptTokens: 8_000,
        toolSchemaTokens: 3_000,
        messageTokens: 150_000,
      },
      messages: [{ role: "user", content: "heavy tools" }],
    },
    {
      compact: () => {
        compacted = true;
        return { compactedMessages: [], compactedTokens: 110_000 };
      },
      send: () => {
        sent = true;
        return { ok: true };
      },
    }
  );

  assert.equal(
    compacted,
    true,
    "Overhead from system prompt and tool schemas must be included, triggering compaction"
  );
  assert.equal(sent, true);
});

test("oversized incoming message or tool result is handled", async () => {
  // A single large message or tool result pushing effective input above cap triggers compaction
  const model = { provider: "openai", id: "gpt-4o", contextWindow: 128_000 };
  // Cap is 102,400.
  let compacted = false;

  await guardModelDispatch(
    {
      model,
      breakdown: {
        systemPromptTokens: 2_000,
        toolSchemaTokens: 1_000,
        messageTokens: 10_000,
        toolResultTokens: 100_000, // 2000 + 1000 + 10000 + 100000 = 113,000 > 102,400
      },
      messages: [{ role: "tool", content: "large tool output" }],
    },
    {
      compact: () => {
        compacted = true;
        return { compactedMessages: [{ role: "tool", content: "truncated" }], compactedTokens: 50_000 };
      },
      send: () => ({ ok: true }),
    }
  );

  assert.equal(compacted, true, "Oversized tool result pushing input over cap must trigger compaction");
});

test("model downswitch recalculates cap before next request", async () => {
  // First request on 1M window model (cap = 200,000) with 150,000 tokens (fits under 200,000)
  const model1M = { provider: "anthropic", id: "claude-3-opus", contextWindow: 1_000_000 };
  let compactCalls = 0;
  let sentCalls = 0;

  await guardModelDispatch(
    { model: model1M, totalTokens: 150_000, messages: [{ role: "user", content: "m1" }] },
    {
      compact: () => {
        compactCalls++;
        return { compactedMessages: [], compactedTokens: 80_000 };
      },
      send: () => {
        sentCalls++;
        return { ok: true };
      },
    }
  );
  assert.equal(compactCalls, 0, "150K tokens on 1M model is under cap (200K), no compaction");
  assert.equal(sentCalls, 1);

  // Model downswitches to 128K window model (cap = 102,400).
  // The next request with same 150,000 tokens must recalculate cap at dispatch and compact!
  const model128K = { provider: "anthropic", id: "claude-3-haiku", contextWindow: 128_000 };
  await guardModelDispatch(
    { model: model128K, totalTokens: 150_000, messages: [{ role: "user", content: "m1" }] },
    {
      compact: () => {
        compactCalls++;
        return { compactedMessages: [], compactedTokens: 80_000 };
      },
      send: () => {
        sentCalls++;
        return { ok: true };
      },
    }
  );
  assert.equal(compactCalls, 1, "Downswitched model must recalculate cap and compact when tokens > new cap");
  assert.equal(sentCalls, 2);
});

test("oversized input after compaction refuses dispatch with explicit error", async () => {
  const model = { provider: "anthropic", id: "claude-3-haiku", contextWindow: 128_000 };
  // Cap is 102,400. Input starts at 150,000. Compaction only reduces it to 110,000 (still > 102,400).
  let compactCalls = 0;
  let sendCalls = 0;

  await assert.rejects(
    async () => {
      await guardModelDispatch(
        { model, totalTokens: 150_000, messages: [{ role: "user", content: "oversized" }] },
        {
          compact: () => {
            compactCalls++;
            return { compactedMessages: [{ role: "user", content: "still oversized" }], compactedTokens: 110_000 };
          },
          send: () => {
            sendCalls++;
            return { ok: true };
          },
        }
      );
    },
    (err: unknown) => {
      assert(err instanceof InputOverCapError || (err instanceof Error && (err as InputOverCapError).code === "input_over_cap"));
      const e = err as InputOverCapError;
      assert.equal(e.code, "input_over_cap");
      assert.equal(e.exitCode, 3);
      assert.equal(e.details.model, "claude-3-haiku");
      assert.equal(e.details.window, 128_000);
      assert.equal(e.details.cap, 102_400);
      assert.equal(e.details.tokens, 110_000);
      return true;
    }
  );

  assert.equal(compactCalls, 1, "Exactly one compaction attempt should run");
  assert.equal(sendCalls, 0, "No model dispatch request should be sent when input remains over cap");
});

test("stored transcript is preserved in full", async () => {
  const originalTranscript = [
    { role: "user", content: "message 1" },
    { role: "assistant", content: "response 1" },
    { role: "user", content: "message 2" },
  ];
  // Deep clone to ensure stored transcript immutability
  const storedTranscript = JSON.parse(JSON.stringify(originalTranscript));

  const model = { provider: "anthropic", id: "claude-3-opus", contextWindow: 128_000 };

  await guardModelDispatch(
    { model, totalTokens: 120_000, messages: storedTranscript },
    {
      compact: (msgs) => {
        // Compaction returns a subset / summarized rebuilt context for the request
        return {
          compactedMessages: [{ role: "system", content: "summary of earlier turns" }, msgs[msgs.length - 1]],
          compactedTokens: 50_000,
        };
      },
      send: () => ({ ok: true }),
    }
  );

  assert.deepEqual(
    storedTranscript,
    originalTranscript,
    "The stored transcript must remain unmodified after compaction"
  );
});

test("invalid or missing context window metadata fails loud", async () => {
  const invalidValues = [
    undefined,
    null,
    0,
    -100,
    1.5,
    NaN,
    Infinity,
    -Infinity,
    "128000",
  ];

  for (const badValue of invalidValues) {
    // 1. calculateModelInputCap fails loud
    assert.throws(
      () => calculateModelInputCap(badValue, "test-model"),
      (err: unknown) => {
        assert(
          err instanceof ContextWindowInvalidError ||
            (err instanceof Error && (err as ContextWindowInvalidError).code === "context_window_invalid")
        );
        const e = err as ContextWindowInvalidError;
        assert.equal(e.code, "context_window_invalid");
        assert.equal(e.exitCode, 1);
        assert.equal(e.details.model, "test-model");
        return true;
      },
      `calculateModelInputCap must reject invalid contextWindow: ${String(badValue)}`
    );

    // 2. guardModelDispatch fails loud without sending any request
    let sendCalls = 0;
    let compactCalls = 0;
    await assert.rejects(
      async () => {
        await guardModelDispatch(
          {
            model: { provider: "test", id: "test-model", contextWindow: badValue },
            totalTokens: 1_000,
            messages: [],
          },
          {
            compact: () => {
              compactCalls++;
              return { compactedMessages: [], compactedTokens: 500 };
            },
            send: () => {
              sendCalls++;
              return { ok: true };
            },
          }
        );
      },
      (err: unknown) => {
        assert(
          err instanceof ContextWindowInvalidError ||
            (err instanceof Error && (err as ContextWindowInvalidError).code === "context_window_invalid")
        );
        const e = err as ContextWindowInvalidError;
        assert.equal(e.code, "context_window_invalid");
        assert.equal(e.exitCode, 1);
        return true;
      },
      `guardModelDispatch must reject invalid contextWindow: ${String(badValue)}`
    );
    assert.equal(sendCalls, 0, "No model call when window is invalid");
    assert.equal(compactCalls, 0, "No compaction when window is invalid");
  }
});

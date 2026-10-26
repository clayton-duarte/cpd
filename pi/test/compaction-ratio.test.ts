/**
 * The compaction ratio (docs/commands.md section 15.19, hs-d660 corrected by hs-d662).
 *
 * `pi/lib/compaction-ratio.ts` hooks pi's `SettingsManager.prototype.getCompactionReserveTokens` so
 * that, for a model with a positive contextWindow, the reserve is `Math.round(contextWindow * ratio)`
 * instead of the flat `compaction.reserveTokens`. The ratio is, per model key `provider/modelId`,
 * highest first: the model override's `reserveTokens` (explicit absolute), the model override's
 * `reserveTokensRatio`, the global `reserveTokensRatio`, the global `compaction.reserveTokens`
 * (explicit absolute), then the built-in `0.1`. A model with no positive contextWindow keeps pi's
 * value; a ratio outside (0, 1) or not finite throws naming the exact key. The value is derived on
 * every call and never stored.
 *
 * These tests are written first, against a stub that installs no hook, so every derived value fails
 * for the right reason until the implementation Job replaces `pi/lib/compaction-ratio.ts`.
 */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { SettingsManager, shouldCompact } from "@earendil-works/pi-coding-agent";
import { registerCompactionRatio } from "../lib/compaction-ratio.ts";

// The user's own settings.json must not decide what these tests derive: an empty agent directory of their own.
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "compaction-ratio-agent-"));
test.after(() => rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }));

/** The model whose key the overrides name. `contextWindow` is omitted for the scope test. */
const model = (contextWindow?: number) => ({
	provider: "openrouter",
	id: "deepseek/deepseek-v4.1-flash",
	contextWindow,
});

/** An in-memory manager over a `compaction` setting pi's schema does not know yet; cast the argument. */
const manager = (compaction: unknown) => SettingsManager.inMemory({ compaction } as never);

/** The derived reserve for a compaction setting and a model. */
const reserve = (compaction: unknown, m: ReturnType<typeof model>) =>
	manager(compaction).getCompactionSettings(m).reserveTokens;

// The hook is installed once, as the adapter installs it; nothing here is stored.
registerCompactionRatio();

test("the default ratio reserves a tenth of the model's context window", () => {
	assert.equal(reserve({}, model(1_000_000)), 100_000);
	assert.equal(reserve({}, model(128_000)), 12_800);
	const settings = manager({}).getCompactionSettings(model(1_000_000));
	assert.equal(shouldCompact(900_001, 1_000_000, settings), true);
	assert.equal(shouldCompact(900_000, 1_000_000, settings), false);
});

test("an explicit global ratio replaces the default", () => {
	assert.equal(reserve({ reserveTokensRatio: 0.2 }, model(1_000_000)), 200_000);
	const settings = manager({ reserveTokensRatio: 0.2 }).getCompactionSettings(model(1_000_000));
	assert.equal(shouldCompact(800_001, 1_000_000, settings), true);
	assert.equal(shouldCompact(800_000, 1_000_000, settings), false);
});

test("a model override ratio wins over the global ratio", () => {
	const compaction = {
		reserveTokensRatio: 0.2,
		modelOverrides: { "openrouter/deepseek/deepseek-v4.1-flash": { reserveTokensRatio: 0.5 } },
	};
	assert.equal(reserve(compaction, model(1_000_000)), 500_000);
});

test("an explicit reserveTokens override wins over every ratio", () => {
	const overriding = {
		reserveTokensRatio: 0.2,
		modelOverrides: {
			"openrouter/deepseek/deepseek-v4.1-flash": { reserveTokens: 8_000, reserveTokensRatio: 0.5 },
		},
	};
	assert.equal(reserve(overriding, model(1_000_000)), 8_000);
	// Without the absolute value the same model ratio wins over the global one, so the absolute is what wins.
	const ratioOnly = {
		reserveTokensRatio: 0.2,
		modelOverrides: { "openrouter/deepseek/deepseek-v4.1-flash": { reserveTokensRatio: 0.5 } },
	};
	assert.equal(reserve(ratioOnly, model(1_000_000)), 500_000);
});

test("an explicit global reserveTokens wins over the default ratio", () => {
	assert.equal(reserve({ reserveTokens: 16_384 }, model(1_000_000)), 16_384);
	assert.equal(reserve({ reserveTokens: 16_384, reserveTokensRatio: 0.2 }, model(1_000_000)), 200_000);
});

test("a model with no context window keeps pi's reserveTokens", () => {
	const compaction = { reserveTokensRatio: 0.2 };
	assert.equal(reserve(compaction, model()), 16_384);
	assert.equal(reserve(compaction, model(0)), 16_384);
});

test("a ratio outside zero and one is refused", () => {
	for (const ratio of [0, 1, -0.1, 1.5, NaN]) {
		assert.throws(
			() => manager({ reserveTokensRatio: ratio }).getCompactionSettings(model(1_000_000)),
			(error: unknown) => error instanceof Error && error.message.includes("compaction.reserveTokensRatio"),
			`ratio ${String(ratio)}`,
		);
	}
	assert.throws(
		() =>
			manager({
				modelOverrides: {
					"openrouter/deepseek/deepseek-v4.1-flash": { reserveTokensRatio: 1.5 },
				},
			}).getCompactionSettings(model(1_000_000)),
		(error: unknown) =>
			error instanceof Error &&
			error.message.includes('compaction.modelOverrides["openrouter/deepseek/deepseek-v4.1-flash"].reserveTokensRatio'),
	);
});

test("compaction triggers at the window minus the derived reserve", () => {
	const settings = manager({ reserveTokensRatio: 0.8 }).getCompactionSettings(model(1_000_000));
	assert.equal(settings.reserveTokens, 800_000);
	assert.equal(shouldCompact(200_001, 1_000_000, settings), true);
	assert.equal(shouldCompact(200_000, 1_000_000, settings), false);
});

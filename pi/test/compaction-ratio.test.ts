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
 * It also hooks three AgentSession methods so threshold auto-compaction runs only while the agent is
 * idle (docs/commands.md section 15.19; context records pro-d353, pro-d204): the idle rules are pinned
 * below against stand-in prototypes through `hookIdleCompaction`.
 *
 * These tests are written first, against a stub that installs no hook, so every derived value fails
 * for the right reason until the implementation Job replaces `pi/lib/compaction-ratio.ts`.
 */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { AgentSession, SettingsManager, shouldCompact } from "@earendil-works/pi-coding-agent";
import { hookIdleCompaction, registerCompactionRatio } from "../lib/compaction-ratio.ts";

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

// The AgentSession methods as pi ships them, captured before the hook is installed.
const piProto = AgentSession.prototype as unknown as Record<string, unknown>;
const IDLE_METHODS = ["_compactBeforeNextAssistantResponse", "_exceedsCompactionThreshold", "_checkCompaction"];
const piOriginals = IDLE_METHODS.map((name) => piProto[name]);

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

// ---- Idle-only threshold compaction (docs/commands.md section 15.19) ----

type Spy = ((...args: unknown[]) => unknown) & { calls: unknown[][] };
const spy = (result: unknown): Spy => {
	const fn = ((...args: unknown[]) => {
		fn.calls.push(args);
		return typeof result === "function" ? (result as (...a: unknown[]) => unknown)(...args) : result;
	}) as Spy;
	fn.calls = [];
	return fn;
};

interface Rig {
	session: Record<string, (...args: unknown[]) => unknown>;
	original: { before: Spy; exceeds: Spy; check: Spy };
	exceeds: Spy;
	run: Spy;
}

/** A fresh stand-in prototype hooked by `hookIdleCompaction`, and an instance with fake collaborators. */
const rig = (opts: { streaming?: boolean; queued?: boolean; checkResult?: boolean; over?: boolean; messages?: unknown[] }): Rig => {
	const original = {
		before: spy((context: unknown) => context),
		exceeds: spy(opts.over ?? false),
		check: spy(opts.checkResult ?? false),
	};
	const run = spy(Promise.resolve(true));
	const proto = {
		_compactBeforeNextAssistantResponse: original.before,
		_exceedsCompactionThreshold: original.exceeds,
		_checkCompaction: original.check,
		_runAutoCompaction: run,
	};
	hookIdleCompaction(proto);
	// The instance-level answer the post run fallback consults: the stand-in's own threshold check.
	const session = Object.create(proto, {
		agent: { value: { state: { isStreaming: opts.streaming ?? false }, hasQueuedMessages: () => opts.queued ?? false } },
		sessionManager: { value: { buildSessionProjection: () => ({ messages: opts.messages ?? [] }) } },
		model: { value: model(1_000_000) },
	});
	return { session, original, exceeds: original.exceeds, run };
};

const msg = (stopReason: string) => ({ role: "assistant", stopReason, content: [] });

test("the pre response hook refreshes messages from the session projection and never runs auto compaction", async () => {
	const messages = [{ role: "user", content: "hi" }];
	const { session, run } = rig({ messages, over: true });
	const context = { systemPrompt: "s", messages: [], tools: [] };
	const out = await session._compactBeforeNextAssistantResponse(context);
	assert.deepEqual(out, { ...context, messages });
	assert.equal(run.calls.length, 0);
});

test("the threshold check is false while the agent is streaming even over the threshold", () => {
	const { session, original } = rig({ streaming: true, over: true });
	assert.equal(session._exceedsCompactionThreshold(model(1_000_000), { messages: [] }), false);
	assert.equal(original.exceeds.calls.length, 0);
});

test("the threshold check delegates to the original check when the agent is idle", () => {
	for (const over of [true, false]) {
		const { session, original } = rig({ streaming: false, over });
		const projection = { messages: [] };
		assert.equal(session._exceedsCompactionThreshold(model(1_000_000), projection), over);
		assert.equal(original.exceeds.calls.length, 1);
	}
});

test("a pre run check with skipAbortedCheck false returns false without compacting", async () => {
	const { session, original, run } = rig({ checkResult: true, over: true });
	assert.equal(await session._checkCompaction(msg("stop"), false), false);
	assert.equal(original.check.calls.length, 0);
	assert.equal(run.calls.length, 0);
});

test("a post run check with queued messages returns false without compacting for a non error message", async () => {
	const { session, original, run } = rig({ queued: true, checkResult: true, over: true });
	assert.equal(await session._checkCompaction(msg("stop")), false);
	assert.equal(original.check.calls.length, 0);
	assert.equal(run.calls.length, 0);
});

test("a post run check with queued messages delegates to the original check for an error message", async () => {
	const { session, original, run } = rig({ queued: true, checkResult: true });
	assert.equal(await session._checkCompaction(msg("error")), true);
	assert.equal(original.check.calls.length, 1);
	assert.equal(run.calls.length, 0);
});

test("a post run check with no queued messages compacts on the threshold when the original check declines", async () => {
	const { session, run } = rig({ queued: false, checkResult: false, over: true });
	assert.equal(await session._checkCompaction(msg("stop")), true);
	assert.deepEqual(run.calls, [["threshold", false]]);
});

test("a post run check with no queued messages returns the original result without a second compaction when the original check compacted", async () => {
	const { session, run } = rig({ queued: false, checkResult: true, over: true });
	assert.equal(await session._checkCompaction(msg("stop")), true);
	assert.equal(run.calls.length, 0);
});

test("a post run check with no queued messages does not compact for an error or aborted message when the original check declines", async () => {
	for (const stopReason of ["error", "aborted"]) {
		const { session, run } = rig({ queued: false, checkResult: false, over: true });
		assert.equal(await session._checkCompaction(msg(stopReason)), false);
		assert.equal(run.calls.length, 0);
	}
});

test("a post run check with no queued messages does not compact when the projection is under the threshold", async () => {
	const { session, run } = rig({ queued: false, checkResult: false, over: false });
	assert.equal(await session._checkCompaction(msg("stop")), false);
	assert.equal(run.calls.length, 0);
});

test("registerCompactionRatio hooks the AgentSession prototype once and a second call stacks nothing", () => {
	const hooked = IDLE_METHODS.map((name) => piProto[name]);
	hooked.forEach((fn, i) => {
		assert.equal(typeof fn, "function");
		assert.notEqual(fn, piOriginals[i]);
	});
	registerCompactionRatio();
	IDLE_METHODS.forEach((name, i) => assert.equal(piProto[name], hooked[i]));
});

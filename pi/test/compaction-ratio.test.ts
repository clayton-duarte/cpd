/** Compaction derives its reserve from the active model input cap (air-d213). */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { AgentSession, SettingsManager, shouldCompact } from "@earendil-works/pi-coding-agent";
import { hookIdleCompaction, registerCompactionRatio } from "../lib/compaction-ratio.ts";
import { compactionPolicy } from "../durable-host/compaction.ts";

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

test("legacy ratio and absolute settings do not override the model input cap", () => {
	const settings = [
		{}, { reserveTokensRatio: 0.2 }, { reserveTokens: 16_384 },
		{ reserveTokensRatio: 0, modelOverrides: { "openrouter/deepseek/deepseek-v4.1-flash": { reserveTokens: 8_000, reserveTokensRatio: 1.5 } } },
	];
	for (const setting of settings) {
		assert.equal(reserve(setting, model(1_000_000)), 800_000);
		assert.equal(reserve(setting, model(128_000)), 25_600);
	}
});

test("invalid or missing model windows fail closed", () => {
	for (const window of [undefined, 0, -1, 1.5, NaN, Infinity]) {
		assert.throws(() => reserve({}, model(window)), { code: "context_window_invalid" });
	}
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
const rig = (opts: { enabled?: boolean; streaming?: boolean; queued?: boolean; checkResult?: boolean; over?: boolean; messages?: unknown[] }): Rig => {
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
		autoCompactionEnabled: { value: opts.enabled ?? true },
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

// Exercise the installed production hook with a fake Pi settings receiver, and the durable
// host's live policy with fake model metadata. Neither test starts Pi or a session.
test("Manager compaction threshold follows the model input cap", async (t) => {
	const cases = [
		{ window: 250_000, threshold: 200_000, reserve: 50_000 },
		{ window: 1_000_000, threshold: 200_000, reserve: 800_000 },
		{ window: 123_457, threshold: 98_765, reserve: 24_692 },
	];
	await t.test("installed settings hook uses the current model", async (t) => {
		const fakePi = { getSettings: () => ({ compaction: {} }) };
		const hooked = SettingsManager.prototype.getCompactionReserveTokens as unknown as (
			this: typeof fakePi, m: ReturnType<typeof model>,
		) => number;
		for (const c of cases) {
			await t.test(`window ${c.window}`, () => {
				const reserve = hooked.call(fakePi, model(c.window));
				assert.equal(reserve, c.reserve);
				assert.equal(c.window - reserve, c.threshold);
			});
		}
	});
	await t.test("durable policy follows active model changes", async (t) => {
		let active = { provider: "mock", modelId: "initial" };
		const windows = new Map(cases.map((c, i) => [`mock/model-${i}`, c.window]));
		const source = {
			settings: () => ({}),
			agent: () => active,
			windowOf: (provider: string, modelId: string) => windows.get(`${provider}/${modelId}`),
		};
		for (const [i, c] of cases.entries()) {
			active = { provider: "mock", modelId: `model-${i}` };
			await t.test(`active model ${i}`, () => {
				const reserve = compactionPolicy(source).reserveTokens;
				assert.equal(reserve, c.reserve);
				assert.equal(c.window - reserve!, c.threshold);
			});
		}
	});
});

test("disabled automatic compaction is respected by the idle fallback", async () => {
	const { session, run } = rig({ enabled: false, over: true, checkResult: false });
	assert.equal(await session._checkCompaction(msg("stop")), false);
	assert.equal(run.calls.length, 0);
});

/**
 * Balloon render cache (docs/contract.md section 6.9, "Chat balloons", the Render cache bullet): the finished rows of a
 * balloon are kept per Markdown child and served again while nothing they depend on changed.
 */
import "./compact-tools/indicator-harness.ts";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { AssistantMessageComponent, UserMessageComponent } from "@earendil-works/pi-coding-agent";
import { loadExtension, restoreClocks, shutdown, type Harness } from "./compact-tools/indicator-harness.ts";

process.env.TZ = "Asia/Kolkata";
const agentDir = mkdtempSync(join(tmpdir(), "balloon-cache-agent-"));
process.env.PI_CODING_AGENT_DIR = agentDir;
test.after(() => { restoreClocks(); rmSync(agentDir, { recursive: true, force: true }); });

const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");
const SGR = /\x1b\[[0-9;]*m/g;
const OSC = /\x1b\][^\x07]*\x07/g;
const plain = (rows: string[]) => rows.map((row) => row.replace(OSC, "").replace(SGR, ""));

const KEY = Symbol.for("cpd.mentions.painter");
type Painter = (line: string) => string;
const holder = globalThis as unknown as Record<symbol, Painter | undefined>;

interface Markdown { text: string; render(width: number): string[]; invalidate(): void }
interface Probe { renders: number; invalidates: number; reset(): void }

/** Count the calls the balloon makes on the Markdown child of pi's own component (the child is pi's, only watched). */
function watch(component: unknown): { md: Markdown; probe: Probe } {
	const find = (node: { children?: unknown[] }): Markdown | undefined => {
		for (const child of node.children ?? []) {
			if (typeof (child as Markdown).text === "string" && typeof (child as Markdown).render === "function") return child as Markdown;
			const found = find(child as { children?: unknown[] });
			if (found) return found;
		}
		return undefined;
	};
	const md = find(component as { children?: unknown[] });
	assert.ok(md, "the component holds a Markdown child");
	const render = md.render.bind(md);
	const invalidate = md.invalidate.bind(md);
	const probe: Probe = { renders: 0, invalidates: 0, reset() { probe.renders = 0; probe.invalidates = 0; } };
	md.render = (width) => { probe.renders++; return render(width); };
	md.invalidate = () => { probe.invalidates++; invalidate(); };
	return { md, probe };
}

/** A mentions painter that counts its calls. */
function painter(): { paint: Painter; calls: () => number } {
	let calls = 0;
	return { paint: (line) => { calls++; return line; }, calls: () => calls };
}

async function withHarness<T>(body: (harness: Harness) => Promise<T> | T): Promise<T> {
	const harness = await loadExtension({}, { tui: true });
	try {
		return await body(harness);
	} finally {
		shutdown(harness);
	}
}

const agentMessage = (text = "ping abc12345") =>
	new AssistantMessageComponent({ role: "assistant", content: [{ type: "text", text }], stopReason: "stop", timestamp: TIME_MS } as never);

async function withPainter<T>(paint: Painter, body: () => Promise<T> | T): Promise<T> {
	holder[KEY] = paint;
	try {
		return await body();
	} finally {
		delete holder[KEY];
	}
}

test("balloon cache: an unchanged user message renders its Markdown and paints mentions once", async () => {
	const counting = painter();
	await withPainter(counting.paint, () => withHarness(() => {
		const message = new UserMessageComponent("ping abc12345");
		const { probe } = watch(message);
		const first = message.render(40);
		assert.equal(probe.renders, 1, "the first frame renders the Markdown");
		const painted = counting.calls();
		assert.ok(painted > 0, "the first frame paints mentions");
		probe.reset();
		assert.deepEqual(message.render(40), first, "the second frame has the same rows");
		assert.deepEqual(message.render(40), first);
		assert.equal(probe.renders, 0, "a cached frame does not render the Markdown");
		assert.equal(probe.invalidates, 0, "a cached frame does not invalidate the Markdown");
		assert.equal(counting.calls(), painted, "a cached frame does not paint mentions");
	}));
});

test("balloon cache: an unchanged agent message renders its Markdown and paints mentions once", async () => {
	const counting = painter();
	await withPainter(counting.paint, () => withHarness(() => {
		const message = agentMessage();
		const { probe } = watch(message);
		const first = message.render(40);
		assert.equal(probe.renders, 1, "the first frame renders the Markdown");
		const painted = counting.calls();
		assert.ok(painted > 0, "the first frame paints mentions");
		probe.reset();
		assert.deepEqual(message.render(40), first);
		assert.deepEqual(message.render(40), first);
		assert.equal(probe.renders, 0, "a cached frame does not render the Markdown");
		assert.equal(probe.invalidates, 0, "a cached frame does not invalidate the Markdown");
		assert.equal(counting.calls(), painted, "a cached frame does not paint mentions");
	}));
});

test("balloon cache: a new pane width draws the balloon again", async () => {
	await withHarness(() => {
		const message = agentMessage("alpha beta gamma delta epsilon zeta");
		const { probe } = watch(message);
		const wide = message.render(60);
		message.render(60);
		assert.equal(probe.renders, 1, "the same width is served from the cache");
		const narrow = message.render(30);
		assert.equal(probe.renders, 2, "a new width renders the Markdown again");
		assert.notDeepEqual(plain(narrow), plain(wide));
		message.render(30);
		assert.equal(probe.renders, 2, "the new width is cached in turn");
	});
});

test("balloon cache: new text draws the balloon again", async () => {
	await withHarness(() => {
		const message = new UserMessageComponent("first words");
		const { md, probe } = watch(message);
		const before = message.render(40);
		message.render(40);
		assert.equal(probe.renders, 1, "the same text is served from the cache");
		md.text = "other words";
		const after = message.render(40);
		assert.equal(probe.renders, 2, "new text renders the Markdown again");
		assert.ok(plain(after).join("\n").includes("other words"));
		assert.notDeepEqual(after, before);
	});
});

test("balloon cache: toggling the chat margins draws the balloon again", async () => {
	const on = await loadExtension({ style: "compact", fold: "toggle" }, { tui: true, defaults: true });
	let off: Harness | undefined;
	try {
		const message = new UserMessageComponent("one two three four five six seven eight nine ten");
		const { probe } = watch(message);
		const margined = message.render(30);
		message.render(30);
		assert.equal(probe.renders, 1, "the same margins are served from the cache");
		off = await loadExtension({ style: "compact", fold: "status" }, { tui: true, defaults: true });
		const plainer = message.render(30);
		assert.equal(probe.renders, 2, "the other margins render the Markdown again");
		assert.notDeepEqual(plain(plainer), plain(margined));
	} finally {
		if (off) shutdown(off);
		shutdown(on);
	}
});

test("balloon cache: a new timestamp draws the agent balloon again", async () => {
	await withHarness(() => {
		const message = agentMessage("a reply wide enough for the time");
		const { probe } = watch(message);
		const before = message.render(60);
		message.render(60);
		assert.equal(probe.renders, 1, "the same timestamp is served from the cache");
		const last = (message as unknown as { lastMessage: object }).lastMessage;
		(message as unknown as { lastMessage: object }).lastMessage = { ...last, timestamp: TIME_MS + 3 * 3_600_000 };
		const after = message.render(60);
		assert.equal(probe.renders, 2, "a new timestamp renders the Markdown again");
		assert.notDeepEqual(plain(after), plain(before));
	});
});

test("balloon cache: a new mentions painter draws the balloon again", async () => {
	const first = painter();
	const second = painter();
	await withPainter(first.paint, () => withHarness(() => {
		const message = agentMessage();
		const { probe } = watch(message);
		message.render(40);
		message.render(40);
		assert.equal(probe.renders, 1, "the same painter is served from the cache");
		holder[KEY] = second.paint;
		message.render(40);
		assert.equal(probe.renders, 2, "a new painter renders the Markdown again");
		assert.ok(second.calls() > 0, "the new painter paints the rows");
		const painted = second.calls();
		message.render(40);
		assert.equal(second.calls(), painted, "the new painter's rows are cached in turn");
	}));
});

test("balloon cache: invalidate on the Markdown draws the balloon again on the next frame", async () => {
	await withHarness(() => {
		const message = new UserMessageComponent("some words");
		const { md, probe } = watch(message);
		message.render(40);
		message.render(40);
		assert.equal(probe.renders, 1, "unchanged frames are served from the cache");
		md.invalidate();
		probe.reset();
		message.render(40);
		assert.equal(probe.renders, 1, "the next frame renders the Markdown again");
		message.render(40);
		assert.equal(probe.renders, 1, "and the frame after is cached again");
	});
});

test("balloon cache: a pane too narrow for a balloon draws the native rows after a balloon", async () => {
	await withHarness(() => {
		const text = "narrow pane words";
		const message = new UserMessageComponent(text);
		const { probe } = watch(message);
		message.render(40);
		message.render(40);
		assert.equal(probe.renders, 1, "the balloon is cached");
		probe.reset();
		const rows = message.render(8);
		assert.equal(probe.renders, 1, "the narrow frame renders the Markdown");
		assert.ok(probe.invalidates >= 1, "the narrow frame invalidates the Markdown first");
		const native = new UserMessageComponent(text).render(8);
		assert.deepEqual(rows, native, "the rows are pi's own, with its padding");
	});
});

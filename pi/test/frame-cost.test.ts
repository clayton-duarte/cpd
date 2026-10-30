/**
 * Frame cost (docs/contract.md section 6.9, "Chat balloons", the Render cache bullet): pi-tui lays the chat out at two
 * widths per frame, so the rows of the two most recently used widths stay cached and a repeated frame segments no text.
 */
import "./compact-tools/indicator-harness.ts";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { afterEach, mock } from "node:test";
import { AssistantMessageComponent, UserMessageComponent } from "@earendil-works/pi-coding-agent";
import { truncateToWidth, type Component } from "@earendil-works/pi-tui";
import { CachedComponent, CachedContainer } from "../compact-tools/extensions/compact-tools-layout.ts";
import { loadExtension, restoreClocks, shutdown, type Harness } from "./compact-tools/indicator-harness.ts";
import { spy } from "./compact-tools/segmenter-spy.ts";

process.env.TZ = "Asia/Kolkata";
const agentDir = mkdtempSync(join(tmpdir(), "frame-cost-agent-"));
process.env.PI_CODING_AGENT_DIR = agentDir;
test.after(() => { restoreClocks(); rmSync(agentDir, { recursive: true, force: true }); });
afterEach(() => mock.restoreAll());

const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");
const WIDE = 60;
const NARROW = 58;

interface Markdown { text: string; render(width: number): string[]; invalidate(): void }

function watch(component: unknown): { md: Markdown; renders: () => number } {
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
	let renders = 0;
	md.render = (width) => { renders++; return render(width); };
	return { md, renders: () => renders };
}

async function withHarness<T>(body: (harness: Harness) => Promise<T> | T): Promise<T> {
	const harness = await loadExtension({}, { tui: true });
	try {
		return await body(harness);
	} finally {
		shutdown(harness);
	}
}

const agentMessage = (text: string) =>
	new AssistantMessageComponent({ role: "assistant", content: [{ type: "text", text }], stopReason: "stop", timestamp: TIME_MS } as never);

const words = (tag: string) => `${tag} ` + Array.from({ length: 30 }, (_, i) => `wörd${i}-${tag}`).join(" ");

type Message = { render(width: number): string[] };

/** One pi-tui frame: every message at the pane width, then every message at the pane width minus the pad. */
function frame(messages: Message[]): void {
	for (const width of [WIDE, NARROW]) for (const message of messages) message.render(width);
}

function chat(count: number, tag: string): { messages: Message[]; mds: Markdown[] } {
	const messages: Message[] = [];
	const mds: Markdown[] = [];
	for (let i = 0; i < count; i++) {
		const text = words(`${tag}-${i}`);
		const message = i % 2 === 0 ? new UserMessageComponent(text) : agentMessage(text);
		mds.push(watch(message).md);
		messages.push(message);
	}
	return { messages, mds };
}

test("frame cost: two alternating widths render an unchanged balloon once per width", async () => {
	await withHarness(() => {
		const message = agentMessage(words("one-balloon"));
		const { renders } = watch(message);
		const wide = message.render(WIDE);
		const narrow = message.render(NARROW);
		for (let i = 0; i < 4; i++) {
			assert.deepEqual(message.render(WIDE), wide);
			assert.deepEqual(message.render(NARROW), narrow);
		}
		assert.equal(renders(), 2, "the Markdown renders once per width");
	});
});

test("frame cost: 50 messages at two alternating widths segment no text after the first frame", async () => {
	await withHarness(() => {
		const { messages } = chat(50, "fifty");
		const probe = spy();
		frame(messages);
		assert.ok(probe.count() > 0, "the first frame segments text");
		const warm = probe.count();
		for (let i = 0; i < 9; i++) frame(messages);
		assert.equal(probe.count(), warm, "frames 2 to 10 segment nothing");
	});
});

test("frame cost: one changed message costs the same segmenting among 10 messages as among 50", async () => {
	await withHarness(() => {
		const cost = (count: number, tag: string): number => {
			const { messages, mds } = chat(count, tag);
			frame(messages);
			const probe = spy();
			mds[3]!.text = words(`${tag}-changed`);
			const before = probe.count();
			frame(messages);
			return probe.count() - before;
		};
		const ten = cost(10, "ten");
		const fifty = cost(50, "fifty-changed");
		assert.ok(ten > 0, "the changed message is segmented");
		assert.equal(fifty, ten);
	});
});

test("frame cost: a CachedComponent serves two alternating widths from its cache", () => {
	const text = "ünïque-cached-component ".repeat(8);
	let calls = 0;
	const component = new CachedComponent((width) => { calls++; return [truncateToWidth(text, width)]; });
	const probe = spy();
	const wide = component.render(WIDE);
	const narrow = component.render(NARROW);
	const warm = probe.count();
	assert.ok(warm > 0, "the first pair segments text");
	for (let i = 0; i < 4; i++) {
		assert.deepEqual(component.render(WIDE), wide);
		assert.deepEqual(component.render(NARROW), narrow);
	}
	assert.equal(calls, 2, "renderLines runs once per width");
	assert.equal(probe.count(), warm, "no segmenting after the first pair");
});

test("frame cost: a CachedContainer serves two alternating widths from its cache", () => {
	const text = "ünïque-cached-container ".repeat(8);
	let calls = 0;
	const child: Component = { render: (width) => { calls++; return [truncateToWidth(text, width)]; }, invalidate() {} };
	const container = new CachedContainer();
	container.addChild(child);
	const probe = spy();
	const wide = container.render(WIDE);
	const narrow = container.render(NARROW);
	const warm = probe.count();
	assert.ok(warm > 0, "the first pair segments text");
	for (let i = 0; i < 4; i++) {
		assert.deepEqual(container.render(WIDE), wide);
		assert.deepEqual(container.render(NARROW), narrow);
	}
	assert.equal(calls, 2, "the child renders once per width");
	assert.equal(probe.count(), warm, "no segmenting after the first pair");
});

test("frame cost: a third width draws once and evicts the least recently used width", async () => {
	await withHarness(() => {
		const message = agentMessage(words("third-width"));
		const { renders } = watch(message);
		const drew = (width: number): number => { const before = renders(); message.render(width); return renders() - before; };
		assert.equal(drew(WIDE), 1);
		assert.equal(drew(NARROW), 1);
		assert.equal(drew(50), 1, "a third width draws once");
		assert.equal(drew(NARROW), 0, "the latest two widths are served");
		assert.equal(drew(50), 0);
		assert.equal(drew(WIDE), 1, "the least recently used width was dropped");
	});
});

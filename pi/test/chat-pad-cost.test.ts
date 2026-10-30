/**
 * Chat pad cost (docs/contract.md section 6.1 "chat pad" and 6.9 "Render cache"): the padded root is laid out at ONE
 * width, so a padded frame segments only the rows on screen, whatever the transcript length.
 */
import "./compact-tools/indicator-harness.ts";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { afterEach, mock } from "node:test";
import { AssistantMessageComponent, UserMessageComponent } from "@earendil-works/pi-coding-agent";
import { Container, ScrollView, VStack, visibleWidth, type Component } from "@earendil-works/pi-tui";
import { renderLayoutFrame } from "@earendil-works/pi-tui/dist/layout.js";
import { paddedRoot } from "../lib/pad.ts";
import { loadExtension, restoreClocks, shutdown, type Harness } from "./compact-tools/indicator-harness.ts";
import { spy } from "./compact-tools/segmenter-spy.ts";

process.env.TZ = "Asia/Kolkata";
const agentDir = mkdtempSync(join(tmpdir(), "chat-pad-cost-agent-"));
process.env.PI_CODING_AGENT_DIR = agentDir;
test.after(() => { restoreClocks(); rmSync(agentDir, { recursive: true, force: true }); });
afterEach(() => mock.restoreAll());

const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");
const PANE = 80;
const HEIGHT = 24;

async function withHarness<T>(body: (harness: Harness) => Promise<T> | T): Promise<T> {
	const harness = await loadExtension({}, { tui: true });
	try {
		return await body(harness);
	} finally {
		shutdown(harness);
	}
}

/** The same shape and length for every index: a fixed-width index and the same word count. */
const words = (index: number) => {
	const tag = String(index).padStart(4, "0");
	return `m${tag} ` + Array.from({ length: 30 }, (_, i) => `wörd${String(i).padStart(2, "0")}-${tag}`).join(" ");
};

const message = (index: number): Component =>
	index % 2 === 0
		? (new UserMessageComponent(words(index)) as unknown as Component)
		: (new AssistantMessageComponent({ role: "assistant", content: [{ type: "text", text: words(index) }], stopReason: "stop", timestamp: TIME_MS } as never) as unknown as Component);

class Editor implements Component {
	text = "";
	render(width: number): string[] { return [`> ${this.text}`.slice(0, width).padEnd(width)]; }
	invalidate(): void {}
}

interface Chat { root: Component; editor: Editor; transcript: ScrollView }

/** The adapter's root: a transcript ScrollView following the end, an editor row below. */
function chat(count: number, wrap: (component: Component, index: number) => Component = (c) => c): Chat {
	const document = new Container();
	for (let i = 0; i < count; i++) document.addChild(wrap(message(i), i));
	const transcript = new ScrollView(document, { follow: "end", primary: true });
	const editor = new Editor();
	const root = new VStack([
		{ component: transcript, basis: 0, grow: 1, shrink: 1, minSize: 1 },
		{ component: editor, basis: "auto", grow: 0, shrink: 1, minSize: 1 },
	]);
	return { root, editor, transcript };
}

/** One frame through pi-tui's per-frame entry (what TuiAltScreen.doRender calls). */
const frame = (root: Component, width = PANE): string[] => renderLayoutFrame(root, width, HEIGHT, () => {}).lines;

function warmThenCount(count: number, between: (c: Chat) => void): number {
	const c = chat(count);
	const padded = paddedRoot(c.root);
	frame(padded);
	between(c);
	const segments = spy();
	frame(padded);
	const calls = segments.count();
	mock.restoreAll();
	return calls;
}

test("chat pad: an unchanged padded frame segments the same text among 30 messages as among 100", async () => {
	await withHarness(() => {
		const few = warmThenCount(30, () => {});
		const many = warmThenCount(1500, () => {});
		assert.ok(few > 0, "the visible rows are drawn");
		assert.equal(many, few);
	});
});

test("chat pad: a key typed in the editor segments the same text among 30 messages as among 100", async () => {
	await withHarness(() => {
		const key = (c: Chat) => { c.editor.text += "x"; };
		const few = warmThenCount(30, key);
		const many = warmThenCount(1500, key);
		assert.ok(few > 0, "the visible rows are drawn");
		assert.equal(many, few);
	});
});

test("chat pad: a padded frame renders every message at one width", async () => {
	await withHarness(() => {
		const widths = new Set<number>();
		const c = chat(50, (component, index) => ({
			render: (width: number) => {
				widths.add(width);
				return component.render(width);
			},
			invalidate: () => component.invalidate(),
		}));
		frame(paddedRoot(c.root));
		assert.deepEqual([...widths], [PANE - 2]);
	});
});

test("chat pad: the pad leaves one blank cell on each side and one blank row above and below", async () => {
	await withHarness(() => {
		for (const width of [80, 81]) {
			const inner = renderLayoutFrame(chat(10).root, width - 2, HEIGHT - 2, () => {}).lines;
			// Rows are ragged (the compositor does not pad to the pane), so every row is compared padded to the pane width.
			const fit = (row: string) => row + " ".repeat(Math.max(0, width - visibleWidth(row)));
			const expected = ["", ...inner.map((row) => ` ${row} `), ""].map(fit);
			// pi-tui's compositor adds an empty reset and hyperlink-close at the seams: not content.
			const seam = /\x1b\[0m\x1b\]8;;\x07/g;
			const rows = renderLayoutFrame(paddedRoot(chat(10).root), width, HEIGHT, () => {}).lines.map((row) => fit(row.replace(seam, "")));
			assert.deepEqual(rows, expected);
		}
	});
});

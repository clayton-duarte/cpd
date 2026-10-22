/**
 * compact-tools draws indexed colours 0-15 only (docs/contract.md 6.1.1 and 6.9, hs-d534, hs-d545):
 * no RGB, no faint, no reverse, no derived value. Rails and metadata are 8, the running dot and
 * the working label 6, fold toggles 7, diff lines text 2 and 1 with no background.
 */
import assert from "node:assert/strict";
import test from "node:test";
import {
	advance, announce, animate, CASES, loadExtension, makeRow, restoreClocks, shutdown, timerClock, type Harness,
} from "./indicator-harness.ts";
import { ProgressController } from "../../compact-tools/extensions/compact-tools-progress.ts";
import { ThinkingCycleController } from "../../compact-tools/extensions/compact-tools-thinking.ts";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";

const ESC = "\x1b";
const SGR = /\x1b\[([0-9;:]*)m/gu;
const readable = (line: string) => line.replaceAll(ESC, "\\e");
const plain = (line: string) => line.replace(SGR, "");

/** A colour: an index 0-15, or an unallowed value spelt out. `undefined` is the terminal default. */
type Colour = number | string | undefined;
type Paint = { fg: Colour; bg: Colour };

/** What one SGR parameter list changes, or the first parameter that no index 0-15 palette allows. */
function apply(params: string, paint: Paint): string | undefined {
	const parts = params === "" ? ["0"] : params.split(";");
	for (let i = 0; i < parts.length; i++) {
		const n = Number(parts[i]);
		if (parts[i]!.includes(":")) return parts[i];
		if (n === 0) { paint.fg = undefined; paint.bg = undefined; }
		else if (n === 1 || n === 4 || (n >= 22 && n <= 24)) continue;
		else if (n >= 30 && n <= 37) paint.fg = n - 30;
		else if (n >= 90 && n <= 97) paint.fg = n - 90 + 8;
		else if (n === 39) paint.fg = undefined;
		else if (n >= 40 && n <= 47) paint.bg = n - 40;
		else if (n >= 100 && n <= 107) paint.bg = n - 100 + 8;
		else if (n === 49) paint.bg = undefined;
		else if (n === 38 || n === 48) {
			if (parts[i + 1] !== "5") return parts.slice(i).join(";");
			const index = Number(parts[i + 2]);
			if (!Number.isInteger(index) || index < 0 || index > 15) return parts.slice(i, i + 3).join(";");
			if (n === 38) paint.fg = index; else paint.bg = index;
			i += 2;
		} else return parts[i];
	}
	return undefined;
}

/** Every SGR the lines carry that the palette refuses, as readable text. */
function refused(lines: string[]): string[] {
	const found: string[] = [];
	for (const line of lines) {
		for (const match of line.matchAll(SGR)) {
			const bad = apply(match[1]!, { fg: undefined, bg: undefined });
			if (bad !== undefined) found.push(`${bad} in ${readable(line)}`);
		}
	}
	return found;
}

/** The paint in force at the first occurrence of `needle` in the line. */
function paintAt(line: string, needle: string): Paint {
	const at = plain(line).indexOf(needle);
	assert.ok(at >= 0, `${needle} is in ${readable(line)}`);
	const paint: Paint = { fg: undefined, bg: undefined };
	let visible = 0;
	let last = 0;
	for (const match of line.matchAll(SGR)) {
		const text = line.slice(last, match.index);
		if (visible + [...text].length > at) break;
		visible += [...text].length;
		last = match.index + match[0].length;
		apply(match[1]!, paint);
	}
	const rest = [...line.slice(last)];
	assert.ok(rest.length > 0 || visible === at);
	return paint;
}

function indexedAt(line: string, needle: string, expected: number): void {
	assert.equal(paintAt(line, needle).fg, expected, `${needle} is colour ${expected} in ${readable(line)}`);
}

const NERD = { expand: "\u{F004B}", collapse: "\u{F0063}" };

/** One tool row in the fold language's defaults, driven as Pi drives it, drawn at each stage. */
function stages(harness: Harness, name: string, id: string, failed: boolean, expanded: boolean) {
	const outcome = CASES[name]!;
	announce(harness, id, name);
	const row = makeRow(name, id, outcome.args, harness.definitions.get(name));
	row.setExpanded(expanded);
	const pending = row.render(100);
	row.setArgsComplete();
	harness.handlers.get("tool_execution_start")?.({ toolCallId: id, toolName: name, args: outcome.args });
	row.markExecutionStarted();
	const frames: string[] = [];
	animate([row], 4, 100, (frame) => frames.push(...frame));
	const running = row.render(100);
	advance(1_000);
	harness.handlers.get("tool_execution_end")?.({ toolCallId: id, toolName: name, isError: failed });
	row.updateResult({ ...(failed ? outcome.failure : outcome.success), isError: failed } as never, false);
	return { pending, running, frames, done: row.render(100) };
}

async function withHarness<T>(body: (harness: Harness) => T | Promise<T>): Promise<T> {
	const harness = await loadExtension({ style: "compact" }, { defaults: true });
	try { return await body(harness); } finally { shutdown(harness); }
}

let sequence = 0;
const nextId = () => `palette-${++sequence}`;

/** Thinking, rendered as the extension's markdown transformer: folded, then opened with Ctrl+T. */
function thinkingViews(): { folded: string; expanded: string } {
	let transform: ((markdown: string, context: unknown) => string) | undefined;
	let terminalInput: ((data: string) => unknown) | undefined;
	const pi = { on: () => {}, registerMarkdownTransformer: (handler: typeof transform) => { transform = handler; } } as unknown as ExtensionAPI;
	const controller = new ThinkingCycleController(pi, () => true, () => NERD);
	controller.bind({
		ui: {
			theme,
			setHiddenThinkingLabel: () => {},
			onTerminalInput: (handler: typeof terminalInput) => { terminalInput = handler; return () => {}; },
		},
	} as unknown as ExtensionContext);
	const render = () => transform!("**Check the code**\n\nInspect the renderer.\nKeep the cache.", {
		messageType: "assistant-thinking", isStreaming: false, availableWidth: 80,
	});
	const folded = render();
	terminalInput!("\x14");
	const expanded = render();
	controller.dispose();
	return { folded, expanded };
}

/** The working label as the host receives it, frame after frame. */
function workingFrames(): string[] {
	const handlers = new Map<string, (event: unknown) => void>();
	const pi = { on: (name: string, handler: (event: unknown) => void) => handlers.set(name, handler), getThinkingLevel: () => "medium" } as unknown as ExtensionAPI;
	const messages: string[] = [];
	const controller = new ProgressController(pi);
	controller.bind({
		isIdle: () => false,
		ui: {
			theme,
			setWorkingVisible: () => {},
			setWorkingIndicator: () => {},
			setWorkingMessage: (message?: string) => { if (message) messages.push(message); },
		},
	} as unknown as ExtensionContext);
	handlers.get("agent_start")!({});
	timerClock.tick(45 * 40);
	controller.dispose();
	return messages;
}

test("tool rows draw only indexed colours 0-15", async () => {
	await withHarness((harness) => {
		const lines: string[] = [];
		for (const name of ["read", "bash", "grep"]) {
			for (const failed of [false, true]) {
				for (const expanded of [false, true]) {
					const row = stages(harness, name, nextId(), failed, expanded);
					lines.push(...row.pending, ...row.running, ...row.frames, ...row.done);
				}
			}
		}
		assert.ok(lines.some((line) => line.includes("╭─")), "the rows are drawn");
		assert.deepEqual(refused(lines), []);
	});
});

test("the thinking block draws only indexed colours 0-15", () => {
	const { folded, expanded } = thinkingViews();
	assert.ok(plain(folded).includes("Thinking..."), "folded thinking is the one title");
	assert.ok(!plain(folded).includes("Check the code"), "its text is inside the fold");
	assert.ok(plain(expanded).includes("Inspect the renderer."), "expanded thinking is drawn");
	assert.deepEqual(refused([...folded.split("\n"), ...expanded.split("\n")]), []);
});

test("the working indicator draws only indexed colours 0-15", async () => {
	await withHarness((harness) => {
		const frames = workingFrames();
		assert.ok(frames.length > 5, "the label animates over several frames");
		const dots: string[] = [];
		for (const name of ["read", "bash"]) {
			const row = stages(harness, name, nextId(), false, false);
			dots.push(...row.running, ...row.frames);
		}
		assert.deepEqual(refused([...frames, ...dots]), []);
	});
});

test("diff rows draw only indexed colours 0-15", async () => {
	await withHarness((harness) => {
		for (const expanded of [false, true]) {
			const row = stages(harness, "edit", nextId(), false, expanded);
			assert.ok(row.done.some((line) => plain(line).includes("line three")), "the diff is drawn");
			assert.deepEqual(refused([...row.running, ...row.done]), []);
		}
	});
});

test("rails and metadata are colour 8", async () => {
	await withHarness((harness) => {
		for (const name of ["bash", "edit"]) {
			const { done } = stages(harness, name, nextId(), false, true);
			const rails = done.filter((line) => plain(line).includes("\u2502"));
			assert.ok(rails.length > 0, `${name} has rails`);
			for (const line of rails) indexedAt(line, "\u2502", 8);
			for (const line of done.filter((l) => plain(l).includes("\u256D"))) indexedAt(line, "\u256D", 8);
			for (const line of done.filter((l) => plain(l).includes("\u2570"))) indexedAt(line, "\u2570", 8);
			const footer = done.find((line) => plain(line).includes("Done in"))!;
			indexedAt(footer, "Done in", 8);
		}
	});
});

test("the running dot and working label are colour 6", async () => {
	await withHarness((harness) => {
		for (const name of ["read", "bash"]) {
			const id = nextId();
			announce(harness, id, name);
			const row = makeRow(name, id, CASES[name]!.args, harness.definitions.get(name));
			row.setArgsComplete();
			const pending = row.render(100);
			harness.handlers.get("tool_execution_start")?.({ toolCallId: id, toolName: name, args: CASES[name]!.args });
			row.markExecutionStarted();
			const frames: string[] = [];
			animate([row], 6, 100, (frame) => frames.push(...frame));
			assert.ok([...pending, ...frames].every((l) => !plain(l).includes("\u2981")), `${name} draws no dot under the fold toggle`);
			assert.ok([...pending, ...frames].some((l) => plain(l).includes("╭─")), `${name} draws its row`);
		}
		const label = workingFrames();
		assert.ok(label.length > 0);
		for (const message of label) {
			const paint: Paint = { fg: undefined, bg: undefined };
			let last = 0;
			const colours = new Set<Colour>();
			for (const match of message.matchAll(SGR)) {
				const text = message.slice(last, match.index);
				if (plain(text) !== "") colours.add(paint.fg);
				last = match.index + match[0].length;
				apply(match[1]!, paint);
			}
			if (plain(message.slice(last)) !== "") colours.add(paint.fg);
			assert.deepEqual([...colours].sort(), [0, 6], `the label is a pill: caps 6, text 0 on bg 6 in ${readable(message)}`);
		}
	});
});

test("fold toggles are colour 7", async () => {
	await withHarness((harness) => {
		for (const [expanded, glyph, word] of [[false, NERD.expand, "expand"], [true, NERD.collapse, "collapse"]] as const) {
			const { done } = stages(harness, "bash", nextId(), false, expanded);
			const toggle = done.find((line) => plain(line).includes(glyph));
			assert.ok(toggle, `the ${word} toggle is drawn`);
			indexedAt(toggle, glyph, 7);
			indexedAt(toggle, word, 7);
		}
		const { expanded } = thinkingViews();
		const toggle = expanded.split("\n").find((line) => plain(line).includes(NERD.collapse));
		assert.ok(toggle, "the thinking block draws a collapse toggle");
		indexedAt(toggle, "collapse", 7);
	});
});

test("diff added and removed lines are text 2 and 1 with no background", async () => {
	await withHarness((harness) => {
		const { done } = stages(harness, "edit", nextId(), false, true);
		const removed = done.find((line) => plain(line).includes("3 -"));
		const added = done.find((line) => plain(line).includes("3 +"));
		assert.ok(removed && added, "the diff has a removed and an added line");
		for (const [line, colour, texts] of [[removed, 1, ["3 -", "line"]], [added, 2, ["3 +", "line", "three"]]] as const) {
			for (const text of texts) {
				const at = paintAt(line, text);
				assert.equal(at.fg, colour, `${text} is colour ${colour} in ${readable(line)}`);
				assert.equal(at.bg, undefined, `${text} has no background in ${readable(line)}`);
			}
			assert.doesNotMatch(line, /\x1b\[(?:[0-9;]*;)?(?:4[0-8]|10[0-7])(?:;[0-9;]*)?m/u, "no background is set");
		}
	});
});

test.after(() => restoreClocks());

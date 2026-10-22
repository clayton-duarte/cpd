/**
 * Tool pills and the Thinking pill (docs/contract.md sections 6.1.1 and 6.9): a block is its colour as
 * background with text 0 (\x1b[30m), caps U+E0B6 / U+E0B4 in the colour as foreground. Running (and not
 * yet started) cyan 6, done green 2, failed red 1, Thinking grey 7 (hs-d557). Indexed SGR only.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { announce, CASES, loadExtension, makeRow, restoreClocks, shutdown, advance } from "./indicator-harness.ts";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";

const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");
const LEFT = "\uE0B6";
const RIGHT = "\uE0B4";
const ESC = "\x1b[";
const COLOUR = { cyan: 6, green: 2, red: 1, grey: 7 } as const;
const fg = (n: number) => `${ESC}${30 + n}m`;
const bg = (n: number) => `${ESC}${40 + n}m`;
const TEXT0 = `${ESC}30m`;

/** The pill the colour draws around a title: left cap, title on the colour with text 0, right cap. */
function pillAround(colour: number, line: string): void {
	const left = line.indexOf(LEFT);
	assert.ok(left >= 0, `a left cap in ${JSON.stringify(line)}`);
	assert.ok(line.slice(0, left).includes(fg(colour)), `left cap in fg ${colour}: ${JSON.stringify(line)}`);
	const right = line.indexOf(RIGHT, left);
	assert.ok(right > left, `a right cap in ${JSON.stringify(line)}`);
	const body = line.slice(left + 1, right);
	assert.ok(body.includes(bg(colour)), `title on bg ${colour}: ${JSON.stringify(body)}`);
	assert.ok(body.includes(TEXT0), `title text 0: ${JSON.stringify(body)}`);
	assert.ok(line.slice(right).startsWith(RIGHT));
}

let sequence = 0;
type Phase = "before" | "running" | "done" | "failed";

async function headOf(phase: Phase): Promise<string> {
	const harness = await loadExtension({ style: "compact", fold: "toggle" }, { defaults: true });
	try {
		const id = `pill-${++sequence}`;
		const outcome = CASES.bash!;
		announce(harness, id, "bash");
		const row = makeRow("bash", id, outcome.args, harness.definitions.get("bash"));
		row.setArgsComplete();
		if (phase !== "before") {
			harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: "bash", args: outcome.args });
			row.markExecutionStarted();
		}
		if (phase === "done" || phase === "failed") {
			advance(300);
			const failed = phase === "failed";
			harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: failed });
			row.updateResult({ ...(failed ? outcome.failure : outcome.success), isError: failed } as never, false);
		}
		const lines = row.render(100);
		return lines.find((line) => plain(line).trim() !== "")!;
	} finally {
		shutdown(harness);
	}
}

test("a tool pill is cyan while running", async () => {
	pillAround(COLOUR.cyan, await headOf("running"));
});

test("a tool pill is cyan before the tool starts", async () => {
	pillAround(COLOUR.cyan, await headOf("before"));
});

test("a tool pill is green when done", async () => {
	pillAround(COLOUR.green, await headOf("done"));
});

test("a tool pill is red when failed", async () => {
	pillAround(COLOUR.red, await headOf("failed"));
});

test("the caps of a tool pill take its colour on the terminal background", async () => {
	for (const [phase, colour] of [["running", 6], ["done", 2], ["failed", 1]] as const) {
		const head = await headOf(phase);
		const left = head.indexOf(LEFT);
		const right = head.indexOf(RIGHT);
		assert.ok(left >= 0 && right > left, phase);
		// the cap is drawn in the colour as foreground, with no background of its own
		const beforeLeft = head.slice(0, left);
		const lastBg = [...beforeLeft.matchAll(/\x1b\[(\d+)m/gu)].map((m) => Number(m[1])).filter((n) => (n >= 40 && n <= 47) || (n >= 100 && n <= 107) || n === 49 || n === 0);
		assert.ok(lastBg.length === 0 || lastBg.at(-1) === 49 || lastBg.at(-1) === 0, `${phase}: left cap on the terminal background`);
		assert.ok(beforeLeft.endsWith(fg(colour)), `${phase}: left cap fg ${colour}: ${JSON.stringify(head)}`);
		const beforeRight = head.slice(0, right);
		assert.ok(/\x1b\[(0|49)m(\x1b\[[0-9;]*m)*$/u.test(beforeRight) && beforeRight.endsWith(fg(colour)),
			`${phase}: right cap fg ${colour} on the terminal background: ${JSON.stringify(head)}`);
	}
});

// --- thinking ---------------------------------------------------------------------------------

async function thinking(source = "## **Check the implementation**\n\nInspect the renderer.\nKeep the cache.") {
	await loadExtension({ style: "compact", fold: "toggle" }, { defaults: true });
	const compactTools = (await import("../../compact-tools/extensions/compact-tools.ts")).default;
	const transforms: Array<(markdown: string, context: unknown) => string> = [];
	const handlers = new Map<string, Array<(event: unknown, ctx: unknown) => void>>();
	let input: ((data: string) => unknown) | undefined;
	const ui = new Proxy({} as Record<string, unknown>, {
		get: (_target, key) => {
			if (key === "theme") return theme;
			if (key === "onTerminalInput") return (fn: typeof input) => { input = fn; return () => {}; };
			return () => () => {};
		},
	});
	const ctx = { cwd: "/project", mode: "tui", isProjectTrusted: () => false, ui } as unknown as ExtensionContext;
	compactTools({
		on: (name: string, fn: (event: unknown, ctx: unknown) => void) => handlers.set(name, [...(handlers.get(name) ?? []), fn]),
		registerTool() {}, registerCommand() {}, registerShortcut() {},
		registerMarkdownTransformer: (fn: (markdown: string, context: unknown) => string) => transforms.push(fn),
		getThinkingLevel: () => "medium",
	} as unknown as ExtensionAPI);
	handlers.get("session_start")?.forEach((fn) => fn({ reason: "startup" }, ctx));
	const draw = () => transforms.reduce((value, fn) => fn(value, { messageType: "assistant-thinking", isStreaming: false, availableWidth: 80 }), source).split("\n");
	return {
		draw,
		press: () => input?.("\x14"),
		end: () => handlers.get("session_shutdown")?.forEach((fn) => fn({ reason: "quit" }, ctx)),
	};
}

test("the thinking title is the one Thinking pill, colour 7", async () => {
	const block = await thinking();
	try {
		const title = block.draw()[0]!;
		assert.equal(plain(title).trim(), `╭─${LEFT}Thinking…${RIGHT}`);
		pillAround(COLOUR.grey, title);
	} finally { block.end(); }
});

test("a folded thinking block shows the expand toggle and none of its text", async () => {
	const block = await thinking();
	try {
		const lines = block.draw().map(plain).map((line) => line.trim());
		assert.deepEqual(lines, [`╭─${LEFT}Thinking…${RIGHT}`, "\u2570\u{F004B} expand"]);
	} finally { block.end(); }
});

test("toggling a thinking block shows its whole text", async () => {
	const block = await thinking();
	try {
		block.draw();
		block.press();
		const lines = block.draw().map(plain).map((line) => line.trim());
		assert.equal(lines[0], `╭─${LEFT}Thinking…${RIGHT}`);
		assert.ok(lines.includes("\u2502 Inspect the renderer."), JSON.stringify(lines));
		assert.ok(lines.includes("\u2502 Keep the cache."), JSON.stringify(lines));
		assert.equal(lines.at(-1), "\u2570\u{F0063} collapse", JSON.stringify(lines));
	} finally { block.end(); }
});

test.after(() => restoreClocks());

import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { AssistantMessageComponent, initTheme, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { ThinkingCycleController } from "../../compact-tools/extensions/compact-tools-thinking.ts";

import chalk from "chalk";
chalk.level = 3;
initTheme("dark", false);
process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "thinking-hidden-agent-"));
test.after(() => rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }));

const LEFT = "\uE0B6";
const RIGHT = "\uE0B4";
const plain = (line: string) => line.replace(/\x1b\][^\x07]*\x07/gu, "").replace(/\x1b\[[0-9;]*m/gu, "").trim();
const rows = (lines: string[]) => lines.map(plain).filter((line) => line !== "");
const NERD = { expand: "\u{F0B26}", collapse: "\u{F0B2C}" };
const TEXT = "The user wants the pill to expand, so I will look at how the title is built here.\n\nThen decide what the fold should hold.";
const EXPANDED = [
	`╭─${LEFT}Thinking…${RIGHT}`,
	"│ The user wants the pill to expand, so I will look at how the title is built",
	"│ here.",
	"│",
	"│ Then decide what the fold should hold.",
	"╰\u{F0B2C} collapse · 03:11",
	"done",
];
const FOLDED = [`╭─${LEFT}Thinking…${RIGHT}`, "╰\u{F0B26} expand · 03:11", "done"];
const PLAIN = ["Thinking…", "done"];

function live(hideThinkingBlock: boolean) {
	writeFileSync(join(process.env.PI_CODING_AGENT_DIR!, "settings.json"), JSON.stringify({ hideThinkingBlock }));
	let transform: ((markdown: string, context: unknown) => string) | undefined;
	let input: ((data: string) => unknown) | undefined;
	let hide = hideThinkingBlock;
	const pi = { on: () => {}, registerMarkdownTransformer: (h: typeof transform) => { transform = h; } } as unknown as ExtensionAPI;
	const controller = new ThinkingCycleController(pi, () => true, () => NERD);
	const message = new AssistantMessageComponent({
		content: [{ type: "thinking", thinking: TEXT }, { type: "text", text: "done" }], stopReason: "stop", timestamp: TIME_MS,
	} as never, hideThinkingBlock, undefined, undefined, 1, [(m: string, c: unknown) => transform!(m, c)] as never);
	controller.bind({ ui: { theme, setHiddenThinkingLabel: (l?: string) => { if (l !== undefined) message.setHiddenThinkingLabel(l); }, onTerminalInput: (h: typeof input) => { input = h; return () => {}; } } } as unknown as ExtensionContext);
	const draw = () => message.render(80);
	return {
		draw,
		click: () => { const lines = draw(); return (message as any).handleMouse({ type: "click", button: "left", x: 3, y: 1, width: 80, height: lines.length }); },
		// Ctrl+T as pi runs it: when our handler returns undefined, pi flips its flag and tells every message.
		press: () => {
			const result = input!("\x14");
			if (result === undefined) { hide = !hide; message.setHideThinkingBlock(hide); }
			return result;
		},
		end: () => controller.dispose(),
	};
}

test("with hideThinkingBlock true the Thinking block starts folded", () => {
	const block = live(true);
	try {
		assert.deepEqual(rows(block.draw()), FOLDED);
	} finally { block.end(); }
});

test("with hideThinkingBlock true a click expands the block and a second click folds it", () => {
	const block = live(true);
	try {
		block.draw();
		block.click();
		assert.deepEqual(rows(block.draw()), EXPANDED, "expanded after the first click");
		block.click();
		assert.deepEqual(rows(block.draw()), FOLDED, "folded after the second click");
	} finally { block.end(); }
});

test("a click on the plain Thinking… label reached by Ctrl+T expands the block", () => {
	const block = live(false);
	try {
		block.draw();
		block.press();
		block.press();
		block.press();
		assert.deepEqual(rows(block.draw()), PLAIN, "the plain label");
		block.click();
		assert.deepEqual(rows(block.draw()), EXPANDED, "a click expands");
	} finally { block.end(); }
});

test("Ctrl+T cycles folded, expanded, folded, plain label, folded", () => {
	const block = live(false);
	try {
		assert.deepEqual(rows(block.draw()), FOLDED);
		const seen: string[][] = [];
		for (let i = 0; i < 4; i++) { block.press(); seen.push(rows(block.draw())); }
		assert.deepEqual(seen, [EXPANDED, FOLDED, PLAIN, FOLDED]);
	} finally { block.end(); }
});

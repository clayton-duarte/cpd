/**
 * The Thinking fold (docs/contract.md section 6.9, hs-d557 item 1): one title pill `Thinking…`
 * (inverted 7, caps 7), all the thinking text inside the fold, folded `╭─[Thinking…]` then
 * `╰󰬦 expand`, expanded the whole text on rails 8 in text 7 then `╰󰬬 collapse`. Headings stay text.
 * With hideThinkingBlock: true the view still starts folded (pill and expand row); pi's hidden label is only a step of Ctrl+T.
 * Rows may carry the cockpit's one leading space, so rows are compared without it.
 */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { AssistantMessageComponent, initTheme, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { ThinkingCycleController } from "../../compact-tools/extensions/compact-tools-thinking.ts";

initTheme("dark", false);
process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");

// The user's own settings.json (hideThinkingBlock) must not decide the view these tests draw: an empty agent directory of their own.
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "thinking-fold-agent-"));
test.after(() => rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }));

const plain = (line: string) => line.replace(/\x1b\][^\x07]*\x07/gu, "").replace(/\x1b\[[0-9;]*m/gu, "");
const LEFT = "\uE0B6";
const RIGHT = "\uE0B4";
const EXPAND = "\u{F0B26}";
const COLLAPSE = "\u{F0B2C}";
const NERD = { expand: EXPAND, collapse: COLLAPSE };
const ESC = "\x1b[";
const rows = (block: string) => block.split("\n").map((line) => plain(line).trim()).filter((line) => line !== "");

const SOURCE = "Weighing the options.\n\n**Check the code**\nInspect the renderer.\n\n## Next\nKeep the cache.";
const PARAGRAPH = "The user wants the pill to be one and to expand, so I will look at how the title is built and then decide what the fold should hold.";

/**
 * The block as pi draws it: the AssistantMessageComponent renders the thinking via the registered
 * transformer, so the updateContent wrapper (thinkingBehavior) reads the message's timestamp and the
 * toggle row carries the local time. press() is Ctrl+T as the user presses it.
 */
function thinking(source: string) {
	let transform: ((markdown: string, context: unknown) => string) | undefined;
	let terminalInput: ((data: string) => unknown) | undefined;
	const pi = { on: () => {}, registerMarkdownTransformer: (handler: typeof transform) => { transform = handler; } } as unknown as ExtensionAPI;
	const controller = new ThinkingCycleController(pi, () => true, () => NERD);
	let message: AssistantMessageComponent | undefined;
	controller.bind({
		ui: {
			theme,
			setHiddenThinkingLabel: (label?: string) => { if (label !== undefined) message?.setHiddenThinkingLabel(label); },
			onTerminalInput: (handler: typeof terminalInput) => { terminalInput = handler; return () => {}; },
		},
	} as unknown as ExtensionContext);
	message = new AssistantMessageComponent(
		{ content: [{ type: "thinking", thinking: source }], stopReason: "stop", timestamp: TIME_MS } as never,
		false, undefined, undefined, 1,
		[(m: string, c: unknown) => transform!(m, c)] as never,
	);
	const draw = () => message!.render(80);
	return { draw, press: () => terminalInput!("\x14"), end: () => controller.dispose() };
}

const nonEmpty = (lines: string[]): string[] => lines.filter((line) => plain(line).trim() !== "");

function folded(source: string): string {
	const block = thinking(source);
	try { return nonEmpty(block.draw()).join("\n"); } finally { block.end(); }
}

function expanded(source: string): string {
	const block = thinking(source);
	try {
		block.draw(); // the controller learns the block has detail on its first draw, as in Pi
		block.press();
		return nonEmpty(block.draw()).join("\n");
	} finally { block.end(); }
}

test("a thinking block folds to one Thinking pill and an expand toggle", () => {
	for (const source of [SOURCE, PARAGRAPH]) {
		assert.deepEqual(rows(folded(source)), [`╭─${LEFT}Thinking…${RIGHT}`, `╰${EXPAND} expand · 03:11`], source);
	}
});

test("the Thinking pill is inverted in 7 with caps 7", () => {
	const head = folded(SOURCE).split("\n").find((line) => line.includes(LEFT))!;
	assert.ok(head, "a head row with a pill");
	const left = head.indexOf(LEFT);
	const right = head.indexOf(RIGHT, left);
	assert.ok(right > left, "a right cap");
	assert.ok(head.slice(0, left).endsWith(`${ESC}37m`), `left cap in fg 7: ${JSON.stringify(head)}`);
	const body = head.slice(left + 1, right);
	assert.ok(body.startsWith(`${ESC}7m`) && body.endsWith(`${ESC}27m${ESC}37m`), `title inverted: ${JSON.stringify(body)}`);
	assert.ok(!body.includes(`${ESC}47m`) && !body.includes(`${ESC}30m`), `no bg 7, no text 0: ${JSON.stringify(body)}`);
	assert.ok(!/\x1b\[(36|46|96|106)m/u.test(head), "no cyan");
});

test("a one-paragraph thinking block is one pill of Thinking, not its text", () => {
	for (const view of [folded(PARAGRAPH), expanded(PARAGRAPH)]) {
		const lines = view.split("\n");
		const pills = lines.filter((line) => line.includes(LEFT));
		assert.equal(pills.length, 1, JSON.stringify(rows(view)));
		assert.equal(plain(pills[0]!).trim(), `╭─${LEFT}Thinking…${RIGHT}`);
	}
	assert.ok(!plain(folded(PARAGRAPH)).includes("pill to be one"), "the text is inside the fold, not shown folded");
});

test("an expanded thinking block shows its whole text on rails 8 in text 7", () => {
	const view = expanded(SOURCE);
	const lines = view.split("\n");
	assert.equal(plain(lines[0]!).trim(), `╭─${LEFT}Thinking…${RIGHT}`);
	for (const text of ["Weighing the options.", "Check the code", "Inspect the renderer.", "Next", "Keep the cache."]) {
		const row = lines.find((line) => plain(line).includes(text));
		assert.ok(row, `${text} is shown: ${JSON.stringify(rows(view))}`);
		assert.match(plain(row).trim(), /^│ /u, `${text} is on a rail`);
		const rail = row.indexOf("│");
		assert.ok(row.slice(0, rail).endsWith(`${ESC}90m`), `rail 8 on ${JSON.stringify(row)}`);
		const at = row.indexOf(text);
		assert.ok(row.slice(rail, at).includes(`${ESC}37m`), `text 7 on ${JSON.stringify(row)}`);
	}
	assert.equal(lines.filter((line) => line.includes(LEFT)).length, 1, "only the title is a pill");
});

test("an expanded thinking block ends with the collapse toggle", () => {
	const lines = expanded(SOURCE).split("\n");
	const last = lines.at(-1)!;
	assert.equal(plain(last).trim(), `╰${COLLAPSE} collapse · 03:11`, "the toggle alone, no hint");
	assert.ok(last.indexOf("╰") > 0 && last.slice(0, last.indexOf("╰")).endsWith(`${ESC}90m`), "╰ is 8");
	const glyph = last.indexOf(COLLAPSE);
	assert.ok(last.slice(last.indexOf("╰"), glyph).includes(`${ESC}37m`), "the toggle is 7");
});

test("headings inside thinking stay text and make no pill", () => {
	for (const view of [folded(SOURCE), expanded(SOURCE)]) {
		assert.equal(view.split(LEFT).length - 1, 1, JSON.stringify(rows(view)));
		assert.ok(!plain(view).includes(`${LEFT}Check the code`) && !plain(view).includes(`${LEFT}Weighing`), "no section pill");
	}
	assert.ok(rows(expanded(SOURCE)).some((row) => row.startsWith("│ ") && row.includes("Check the code")));
});

test("hideThinkingBlock true in settings.json starts the view folded", () => {
	const previous = process.env.PI_CODING_AGENT_DIR;
	const dir = mkdtempSync(join(tmpdir(), "thinking-fold-"));
	process.env.PI_CODING_AGENT_DIR = dir;
	writeFileSync(join(dir, "settings.json"), JSON.stringify({ hideThinkingBlock: true }));
	let label: string | undefined;
	let transform: ((markdown: string, context: unknown) => string) | undefined;
	const pi = { on: () => {}, registerMarkdownTransformer: (handler: typeof transform) => { transform = handler; } } as unknown as ExtensionAPI;
	const controller = new ThinkingCycleController(pi, () => true, () => NERD);
	try {
		controller.bind({
			ui: { theme, setHiddenThinkingLabel: (value?: string) => { label = value; }, onTerminalInput: () => () => {} },
		} as unknown as ExtensionContext);
		const view = transform!(SOURCE, { messageType: "assistant-thinking", isStreaming: false, availableWidth: 80 });
		assert.deepEqual(rows(view), [`╭─${LEFT}Thinking…${RIGHT}`, `╰${EXPAND} expand`], "folded: pill and the expand row");
		assert.ok(label, "the hidden label is set");
		assert.equal(plain(label), "Thinking…");
		assert.ok(!label.includes(LEFT) && !label.includes(RIGHT), "no pill");
		assert.equal(label, `${ESC}37mThinking…${ESC}39m`, "plain text 7: no bold, no faint");
	} finally {
		controller.dispose();
		if (previous === undefined) delete process.env.PI_CODING_AGENT_DIR;
		else process.env.PI_CODING_AGENT_DIR = previous;
		rmSync(dir, { recursive: true, force: true });
	}
	// The first step out of hidden: visible again, a one-paragraph block is the one pill and the toggle.
	assert.deepEqual(rows(folded(PARAGRAPH)), [`╭─${LEFT}Thinking…${RIGHT}`, `╰${EXPAND} expand · 03:11`]);
});

/**
 * The working label in the editor's top border: `╭─[<frame> <words>]──…╮` with the loader inside the pill
 * (contract 6.1.1, 6.9; hs-d475, hs-d478). Only the top border row is asserted; the box has no margin column.
 */
import "./pinned-themes.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { CustomEditor, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { visibleWidth } from "@earendil-works/pi-tui";
import { WorkingStatusIndicator } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/status-indicator.js";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { ProgressController } from "../../compact-tools/extensions/compact-tools-progress.ts";
import { restoreClocks, timerClock } from "./indicator-harness.ts";
import { MOON_FRAMES } from "./loader-frames.ts";

test.after(() => restoreClocks());

const LOADER = MOON_FRAMES;
const LEFT_CAP = "\ue0b6";
const RIGHT_CAP = "\ue0b4";
const strip = (value: string) => value.replace(/\x1b\[[0-9;]*m/gu, "");
/** The pane width the editor renders in; the box spans it. */
const PANE = 60;

type Handler = (event: unknown) => void;
type Indicator = { frames: string[]; intervalMs?: number };

/** A real CustomEditor with the working status Pi would build from what the extension sets. */
function working(label: (start: Handler) => void = (start) => start({})) {
	const handlers = new Map<string, Handler>();
	const pi = { on: (name: string, handler: Handler) => handlers.set(name, handler), getThinkingLevel: () => "medium" } as unknown as ExtensionAPI;
	let message: string | undefined;
	let indicator: Indicator | undefined;
	const controller = new ProgressController(pi);
	controller.bind({
		isIdle: () => false,
		ui: {
			theme,
			setWorkingVisible: () => {},
			setWorkingIndicator: (value?: Indicator) => { indicator = value; },
			setWorkingMessage: (value?: string) => { message = value; },
		},
	} as unknown as ExtensionContext);
	label(handlers.get("agent_start")!);
	const ui = { requestRender: () => {}, terminal: { rows: 40, columns: PANE } };
	const editor = new CustomEditor(ui as never, { borderColor: (text: string) => text } as never, { matches: () => false } as never, { embedWorkingStatus: true } as never);
	const status = new WorkingStatusIndicator(ui as never, message ?? "", indicator as never, (text: string) => text);
	editor.setWorkingStatusIndicator(status);
	return {
		editor,
		/** The top border as drawn. */
		row: (pane = PANE) => editor.render(pane)[0]!,
		dispose: () => { status.dispose(); controller.dispose(); },
	};
}

test("while working the top border is a corner, one line cell, the pill, then line cells to the corner", () => {
	const w = working();
	try {
		const row = strip(w.row());
		assert.match(row, new RegExp(`^╭─${LEFT_CAP}.+${RIGHT_CAP}─+╮$`, "u"));
		assert.equal(visibleWidth(row), PANE);
	} finally { w.dispose(); }
});

test("the pill has caps 6, its text inverted and no padding inside the caps", () => {
	const w = working();
	try {
		const row = w.row();
		assert.match(row, /\x1b\[36m\ue0b6/u, "the left cap is 6");
		assert.match(row, /\x1b\[36m\ue0b4/u, "the right cap is 6");
		assert.ok(row.includes("\x1b[7m") && row.includes("\x1b[27m") && !row.includes("\x1b[30m"), "inverted text");
		const plain = strip(row);
		const inside = plain.slice(plain.indexOf(LEFT_CAP) + 1, plain.indexOf(RIGHT_CAP));
		assert.equal(inside, inside.trim(), "no padding space inside the caps");
		assert.ok(inside.length > 0, "the pill has a body");
	} finally { w.dispose(); }
});

test("the loader frame sits inside the pill in text 0 and steps through the 28 moon_alt phases at 50 ms", () => {
	const w = working();
	try {
		for (let step = 0; step < LOADER.length + 1; step++) {
			const row = w.row();
			const plain = strip(row);
			const body = plain.slice(plain.indexOf(LEFT_CAP) + 1, plain.indexOf(RIGHT_CAP));
			assert.ok(body.startsWith(LOADER[step % 28]!), `frame ${step % 28} opens the pill, got ${JSON.stringify(body)}`);
			assert.ok(!plain.slice(0, plain.indexOf(LEFT_CAP)).match(/[\u{e3c8}-\u{e3e3}\u{f0000}-\u{fffff}]/u), "no loader frame outside the pill");
			const bodyRaw = row.slice(row.indexOf("\x1b[46m"));
			assert.ok(!bodyRaw.slice(0, bodyRaw.indexOf(LOADER[step % 28]!)).includes("\x1b[36m"), "the frame has no colour of its own");
			timerClock.tick(50);
		}
	} finally { w.dispose(); }
});

test("one space separates the loader frame from the words and the words keep their …", () => {
	const w = working();
	try {
		const plain = strip(w.row());
		const body = plain.slice(plain.indexOf(LEFT_CAP) + 1, plain.indexOf(RIGHT_CAP));
		assert.match(body, /^[\u{e3c8}-\u{e3e3}\u{f0000}-\u{fffff}] Thinking…$/u);
	} finally { w.dispose(); }
});

test("no space separates the border from the caps", () => {
	const w = working();
	try {
		const plain = strip(w.row());
		assert.ok(plain.includes(`─${LEFT_CAP}`), "a line cell touches the left cap");
		assert.ok(plain.includes(`${RIGHT_CAP}─`), "a line cell touches the right cap");
		const outside = plain.slice(0, plain.indexOf(LEFT_CAP)) + plain.slice(plain.indexOf(RIGHT_CAP) + 1);
		assert.ok(!outside.includes(" "), "no space outside the pill");
		const inside = plain.slice(plain.indexOf(LEFT_CAP) + 1, plain.indexOf(RIGHT_CAP));
		assert.ok(!inside.startsWith(" ") && !inside.endsWith(" "), "no padding inside the caps");
	} finally { w.dispose(); }
});

test("a narrow pane truncates the words with … inside the pill and keeps the corner", () => {
	const w = working();
	try {
		const row = strip(w.row(24));
		assert.ok(row.endsWith("╮"), "the corner stays");
		assert.equal(visibleWidth(row), 24);
		const body = row.slice(row.indexOf(LEFT_CAP) + 1, row.indexOf(RIGHT_CAP));
		assert.match(body, /^[\u{e3c8}-\u{e3e3}\u{f0000}-\u{fffff}] Thinking…$/u, "this pane still fits the short label");
		const narrow = strip(w.row(14));
		assert.ok(narrow.endsWith("╮"), "the corner stays when narrow");
		assert.equal(visibleWidth(narrow), 14);
		const inside = narrow.slice(narrow.indexOf(LEFT_CAP) + 1, narrow.indexOf(RIGHT_CAP));
		assert.ok(inside.endsWith("…") && !inside.includes("Thinking…"), `the words are cut with …, got ${JSON.stringify(inside)}`);
		assert.ok(inside.includes("Thi"), "the start of the words stays");
	} finally { w.dispose(); }
});

test("an idle editor's top border is plain", () => {
	const ui = { requestRender: () => {}, terminal: { rows: 40, columns: PANE } };
	const editor = new CustomEditor(ui as never, { borderColor: (text: string) => text } as never, { matches: () => false } as never, { embedWorkingStatus: true } as never);
	const row = strip(editor.render(PANE)[0]!);
	assert.equal(row, `╭${"─".repeat(PANE - 2)}╮`);
});

test("the left cap is the third cell of the row: corner, one line cell, cap", () => {
	const w = working();
	try {
		const row = strip(w.editor.render(PANE)[0]!);
		assert.equal(visibleWidth(row), PANE);
		assert.ok(row.startsWith(`╭─${LEFT_CAP}`), `got ${JSON.stringify(row)}`);
		assert.equal([...row].indexOf(LEFT_CAP), 2, "two cells sit between the container's limit and the cap");
	} finally { w.dispose(); }
});

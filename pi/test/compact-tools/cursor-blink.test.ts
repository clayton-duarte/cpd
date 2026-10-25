/** The input cursor: one Dango-index cell that blinks and cycles only while the terminal pane has focus (hs-d649). */
import "./pinned-themes.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { install } from "@sinonjs/fake-timers";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { CustomEditor } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/custom-editor.js";
import { initTheme, theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

process.env.PI_CODING_AGENT_DIR ??= mkdtempSync(join(tmpdir(), "cursor-blink-"));
initTheme("dark", false);

/** Pi-tui's zero-width hardware-cursor marker, and the fake-cursor span pi's TUI later reverses. */
const CURSOR_MARKER = "\x1b_pi:c\x07";
const START = "\x1b_pi:fc\x07";
const END = "\x1b_pi:/fc\x07";
const MAGENTA = "\x1b[35m";
const WHITE = "\x1b[37m";
const GREEN = "\x1b[32m";
const DEFAULT = "\x1b[39m";
/** The SGR that opens and closes the inverse-video highlight trail (hs-d673). */
const REVERSE = "\x1b[7m";
const UNREVERSE = "\x1b[27m";
const readable = (value: string) => value.replace(/\x1b/gu, "\\e");

type Handler = (event: unknown, ctx?: unknown) => void;
type InputHandler = (data: string) => { consume?: boolean; data?: string } | undefined;

interface Session {
	editor: CustomEditor;
	/** The terminal reports focus in (true) or out (false), through the path an extension sees. */
	focus(inside: boolean): void;
	dispose(): void;
}

/** The real CustomEditor with compact-tools loaded, as editor-box.test.ts loads it. */
async function session(options: { text?: string; showHardwareCursor?: boolean } = {}): Promise<Session> {
	const handlers = new Map<string, Handler[]>();
	const inputs: InputHandler[] = [];
	const compactTools = (await import("../../compact-tools/extensions/compact-tools.ts")).default;
	compactTools({
		on: (name: string, handler: Handler) => handlers.set(name, [...(handlers.get(name) ?? []), handler]),
		registerTool: () => {},
		registerMarkdownTransformer: () => {},
		registerCommand: () => {},
		registerShortcut: () => {},
		getThinkingLevel: () => "medium",
	} as unknown as ExtensionAPI);
	const tui = {
		terminal: { rows: 40, columns: 80 },
		requestRender: () => {},
		...(options.showHardwareCursor === true ? { getShowHardwareCursor: () => true } : {}),
	};
	const noop = () => () => {};
	const ui = new Proxy({} as Record<string, unknown>, {
		get: (_target, key) => {
			if (key === "theme") return theme;
			if (key === "onTerminalInput") return (handler: InputHandler) => { inputs.push(handler); return () => {}; };
			if (key === "setWidget") return (_key: string, factory: unknown) => { if (typeof factory === "function") factory(tui); };
			return noop;
		},
	});
	const ctx = { cwd: "/project", mode: "tui", isProjectTrusted: () => false, isIdle: () => true, ui } as unknown as ExtensionContext;
	const emit = (name: string, event: unknown) => { for (const handler of handlers.get(name) ?? []) handler(event, ctx); };
	emit("session_start", { reason: "startup" });
	const keybindings = { matches: () => false } as never;
	const editor = new CustomEditor(tui as never, { borderColor: (text: string) => text, selectList: { selectedPrefix: (t: string) => t, selectedText: (t: string) => t, description: (t: string) => t, scrollInfo: (t: string) => t, noMatch: (t: string) => t } as never }, keybindings, { embedWorkingStatus: true });
	editor.focused = false;
	if (options.text !== undefined) editor.setText(options.text);
	return {
		editor,
		focus(inside) {
			for (const handler of inputs) handler(inside ? "\x1b[I" : "\x1b[O");
		},
		dispose() {
			emit("session_shutdown", { reason: "quit" });
		},
	};
}

/** The row pi renders the cursor on, and the character inside its fake-cursor span. */
function cursorRow(rows: string[]): { row: string; cell: string } {
	const row = rows.find((line) => line.includes(START));
	assert.ok(row, `a fake cursor is drawn: ${readable(rows.join("\n"))}`);
	const start = row.indexOf(START) + START.length;
	const end = row.indexOf(END, start);
	assert.ok(end >= start, `the fake cursor has an end: ${readable(row)}`);
	return { row, cell: row.slice(start, end) };
}

/** The row with all escape sequences (including the cursor markers) removed. */
const plain = (row: string) => row.replace(/\x1b_pi:\/?(?:fc|c)\x07/gu, "").replace(/\x1b\[[0-9;]*m/gu, "");

/** The characters an inverse-video highlight covers, in order, whichever Dango colour opens it (hs-d673). */
const highlighted = (row: string): string =>
	[...row.matchAll(/\x1b\[(?:35|37|32)m\x1b\[7m([^\x1b]*)\x1b\[27m/gu)].map((match) => match[1]!).join("");

test("the focused cursor is a base-5 block", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session({ text: "hi" });
	try {
		s.focus(true);
		const rows = s.editor.render(40);
		const { row, cell } = cursorRow(rows);
		assert.equal(cell, " ", "the empty position after the text is one cell");
		assert.ok(row.includes(`${CURSOR_MARKER}${MAGENTA}${START}`), `the cursor starts with pi's marker and index 5 magenta: ${readable(row)}`);
		assert.ok(row.includes(`${END}${DEFAULT}`), `the cursor resets the foreground after it: ${readable(row)}`);
	} finally { s.dispose(); clock.uninstall(); }
});

test("the cursor blinks off after 500 ms", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session({ text: "hi" });
	try {
		s.focus(true);
		const before = s.editor.render(40);
		cursorRow(before);
		clock.tick(500);
		const rows = s.editor.render(40);
		const row = rows.find((line) => plain(line).includes("hi"));
		assert.ok(row, `the text row is still drawn: ${readable(rows.join("\n"))}`);
		assert.ok(!row!.includes(START) && !row!.includes(END), `the off phase removes the fake-cursor markers: ${readable(row!)}`);
		assert.ok(plain(row!).includes("hi"), "the text stays visible while the cursor is off");
	} finally { s.dispose(); clock.uninstall(); }
});

test("the cursor cycles magenta 5, white 7, green 2", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session();
	try {
		s.focus(true);
		s.editor.render(40);
		const visible = (expected: string, phase: string) => {
			const rows = s.editor.render(40);
			const { row } = cursorRow(rows);
			assert.ok(row.includes(`${expected}${START}`), `${phase}: ${readable(row)}`);
		};
		const off = (phase: string) => {
			const rows = s.editor.render(40);
			assert.ok(!rows.some((row) => row.includes(START) || row.includes(END)), `${phase}: ${readable(rows.join("\n"))}`);
		};
		visible(MAGENTA, "first visible phase is magenta 5");
		clock.tick(500);
		off("after 500 ms");
		clock.tick(500);
		visible(WHITE, "second visible phase is white 7");
		clock.tick(500);
		off("after 1500 ms");
		clock.tick(500);
		visible(GREEN, "third visible phase is green 2");
		clock.tick(500);
		off("after 2500 ms");
		clock.tick(500);
		visible(MAGENTA, "the cycle returns to magenta 5");
	} finally { s.dispose(); clock.uninstall(); }
});

test("no cursor cell is drawn while the pane is unfocused", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session({ text: "hi" });
	try {
		s.focus(false);
		const rows = s.editor.render(40);
		assert.ok(!rows.some((row) => row.includes(START) || row.includes(END) || row.includes(CURSOR_MARKER)), `no cursor cell: ${readable(rows.join("\n"))}`);
		const row = rows.find((line) => plain(line).includes("hi"));
		assert.ok(row, "the text row is still drawn");
		assert.ok(!row!.includes(MAGENTA) && !row!.includes(WHITE) && !row!.includes(GREEN), `no Dango cursor colour: ${readable(row!)}`);
	} finally { s.dispose(); clock.uninstall(); }
});

test("showHardwareCursor leaves the cursor to pi", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const normal = await session();
	const hardware = await session({ showHardwareCursor: true });
	try {
		// The control: with pi's hardware cursor off, the extension paints and blinks the cell.
		normal.focus(true);
		const painted = normal.editor.render(40);
		const control = cursorRow(painted).row;
		assert.ok(control.includes(`${MAGENTA}${START}`), `the software cursor is painted: ${readable(control)}`);

		// With pi's hardware cursor on, the extension must neither paint nor animate the cell.
		hardware.focus(true);
		const before = hardware.editor.render(40);
		clock.tick(500);
		const after = hardware.editor.render(40);
		assert.deepEqual(after, before, "the rows do not change with the blink clock");
		assert.ok(!after.some((row) => row.includes(MAGENTA) || row.includes(WHITE) || row.includes(GREEN)), `no Dango cursor colour: ${readable(after.join("\n"))}`);
		assert.ok(!after.some((row) => row.includes(CURSOR_MARKER)), `the extension adds no hardware marker of its own: ${readable(after.join("\n"))}`);
	} finally { normal.dispose(); hardware.dispose(); clock.uninstall(); }
});

test("a keystroke pauses the blink and highlights the typed character", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval", "Date"] });
	const s = await session({ text: "hi" });
	try {
		s.focus(true);
		s.editor.render(40);
		s.editor.handleInput("a");
		const { row } = cursorRow(s.editor.render(40));
		assert.equal(highlighted(row), "a", `the typed character is highlighted in reverse video: ${readable(row)}`);
		assert.ok(row.includes(`${MAGENTA}${REVERSE}a${UNREVERSE}${DEFAULT}`), `the highlight holds the cursor's magenta and closes before the cursor cell: ${readable(row)}`);
		assert.ok(row.includes(`${MAGENTA}${START}`), `the cursor still paints its held colour: ${readable(row)}`);
		clock.tick(500);
		cursorRow(s.editor.render(40)); // still drawn: the keystroke paused the 500 ms blink
	} finally { s.dispose(); clock.uninstall(); }
});

test("the highlight trail grows with each keystroke and holds the cursor's colour", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval", "Date"] });
	const s = await session();
	try {
		s.focus(true);
		s.editor.render(40);
		let typed = "";
		for (const character of ["a", "b", "c"]) {
			s.editor.handleInput(character);
			typed += character;
			const { row } = cursorRow(s.editor.render(40));
			assert.ok(row.includes(`${MAGENTA}${REVERSE}${typed}${UNREVERSE}${DEFAULT}`), `the trail grew to ${typed} in the cursor's held magenta: ${readable(row)}`);
		}
		assert.equal(highlighted(cursorRow(s.editor.render(40)).row), "abc", "every typed character stays in the trail");
	} finally { s.dispose(); clock.uninstall(); }
});

test("the trail shrinks one character per tick after 500 ms idle and the blink resumes when it collapses", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval", "Date"] });
	const s = await session();
	try {
		s.focus(true);
		s.editor.render(40);
		for (const character of ["a", "b", "c"]) s.editor.handleInput(character);
		assert.equal(highlighted(cursorRow(s.editor.render(40)).row), "abc", "the trail starts three characters long");
		clock.tick(500);
		assert.equal(highlighted(cursorRow(s.editor.render(40)).row), "bc", "after 500 ms idle the trail drops its left character");
		clock.tick(50);
		assert.equal(highlighted(cursorRow(s.editor.render(40)).row), "c", "the next 50 ms tick drops one more");
		clock.tick(50);
		const collapsed = s.editor.render(40);
		assert.equal(highlighted(cursorRow(collapsed).row), "", "the trail has collapsed and the cursor is drawn again");
		clock.tick(500);
		const off = s.editor.render(40);
		assert.ok(!off.some((row) => row.includes(START) || row.includes(END)), `the resumed blink goes off 500 ms later: ${readable(off.join("\n"))}`);
	} finally { s.dispose(); clock.uninstall(); }
});

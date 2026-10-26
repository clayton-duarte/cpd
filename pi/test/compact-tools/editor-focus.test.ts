/**
 * The focus report in the real TUIs: pi's fullscreen renderer (`TuiAltScreen`) consumes `\x1b[I` and `\x1b[O` in its own
 * first input listener, so the box has to see them ahead of it; the main-screen renderer passes them on (contract 6.9
 * "The input widget": 'focus' is the terminal's focus report for the pane the editor lives in).
 * The extension is bound to a real TUI through its real `addInputListener`, never a fake `onTerminalInput`.
 */
import "./pinned-themes.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { TuiAltScreen, TuiMainScreen } from "@earendil-works/pi-tui";
import { CustomEditor } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/custom-editor.js";
import { initTheme, theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

process.env.PI_CODING_AGENT_DIR ??= mkdtempSync(join(tmpdir(), "editor-focus-"));
initTheme("dark", false);

const FOCUS_IN = "\x1b[I";
const FOCUS_OUT = "\x1b[O";
const BORDER = new Set(["╭", "╮", "╰", "╯", "─", "│"]);
const strip = (value: string) => value.replace(/\x1b\[[0-9;]*m/gu, "").replace(/\x1b_pi:\/?(?:fc|c)\x07/gu, "");
const readable = (value: string) => value.replace(/\x1b/gu, "\\e");

type Handler = (event: unknown, ctx?: unknown) => void;

/** The terminal pi-tui drives, as little of it as the TUIs touch without starting. */
function fakeTerminal() {
	return {
		columns: 80, rows: 24, kittyProtocolActive: false,
		start() {}, stop() {}, write() {}, hideCursor() {}, showCursor() {}, clearScreen() {},
		moveBy() {}, clearLine() {}, clearFromCursor() {}, setTitle() {}, drainInput() {},
	};
}

interface Real {
	tui: TuiAltScreen | TuiMainScreen;
	editor: CustomEditor;
	/** What the terminal sends, through the TUI's own input path. */
	send(data: string): void;
	shutdown(): void;
	/** The border colours (SGR param of each box-drawing cell) of the editor as drawn. */
	border(): string[];
}

/** The compact-tools extension bound to a real TUI the way pi's interactive mode binds it: input listeners go to `tui.addInputListener`. */
async function real(kind: "fullscreen" | "main"): Promise<Real> {
	const tui = kind === "fullscreen"
		? new TuiAltScreen(fakeTerminal() as never, false, undefined, {})
		: new TuiMainScreen(fakeTerminal() as never, false, undefined);
	const handlers = new Map<string, Handler[]>();
	const compactTools = (await import("../../compact-tools/extensions/compact-tools.ts")).default;
	compactTools({
		on: (name: string, handler: Handler) => handlers.set(name, [...(handlers.get(name) ?? []), handler]),
		registerTool: () => {},
		registerMarkdownTransformer: () => {},
		registerCommand: () => {},
		registerShortcut: () => {},
		getThinkingLevel: () => "medium",
	} as unknown as ExtensionAPI);
	const noop = () => () => {};
	const ui = new Proxy({} as Record<string, unknown>, {
		get: (_target, key) => {
			if (key === "theme") return theme;
			if (key === "onTerminalInput") return (handler: Parameters<typeof tui.addInputListener>[0]) => tui.addInputListener(handler);
			if (key === "setWidget") return (_key: string, factory: unknown) => { if (typeof factory === "function") factory(tui); };
			return noop;
		},
	});
	const ctx = { cwd: "/project", mode: "tui", isProjectTrusted: () => false, isIdle: () => true, ui } as unknown as ExtensionContext;
	const emit = (name: string, event: unknown) => { for (const handler of handlers.get(name) ?? []) handler(event, ctx); };
	emit("session_start", { reason: "startup" });
	const editor = new CustomEditor(tui as never, { borderColor: (text: string) => text, selectList: { selectedPrefix: (t: string) => t, selectedText: (t: string) => t, description: (t: string) => t, scrollInfo: (t: string) => t, noMatch: (t: string) => t } as never }, { matches: () => false } as never, { embedWorkingStatus: true });
	editor.setText("hi");
	tui.addChild(editor);
	tui.setFocus(editor);
	return {
		tui,
		editor,
		send: (data) => (tui as unknown as { handleTerminalInput(data: string): void }).handleTerminalInput(data),
		shutdown: () => emit("session_shutdown", { reason: "quit" }),
		border() {
			const fgs = new Set<string | undefined>();
			for (const row of editor.render(30)) {
				let fg: string | undefined;
				for (const part of row.replace(/\x1b_pi:\/?(?:fc|c)\x07/gu, "").split(/(\x1b\[[0-9;]*m)/u)) {
					const sgr = /^\x1b\[([0-9;]*)m$/u.exec(part);
					if (sgr) {
						for (const code of (sgr[1] || "0").split(";")) {
							if (code === "0" || code === "39") fg = undefined;
							else if (/^(3[0-7]|9[0-7])$/u.test(code)) fg = code;
						}
						continue;
					}
					for (const ch of part) if (BORDER.has(ch)) fgs.add(fg);
				}
			}
			return [...fgs].map(String);
		},
	};
}

test("in fullscreen mode a focus out report turns the border to colour 15", async () => {
	const s = await real("fullscreen");
	try {
		assert.deepEqual(s.border(), ["97"], "before any report");
		s.send(FOCUS_OUT);
		assert.deepEqual(s.border(), ["97"], readable(s.editor.render(30).join("\n")));
	} finally { s.shutdown(); }
});

test("in fullscreen mode a focus in report turns the border to colour 12", async () => {
	const s = await real("fullscreen");
	try {
		s.send(FOCUS_OUT);
		s.send(FOCUS_IN);
		assert.deepEqual(s.border(), ["94"], "after focus in");
		s.send(FOCUS_OUT);
		assert.deepEqual(s.border(), ["97"], "focus out must have changed it first");
	} finally { s.shutdown(); }
});

test("in main-screen mode the same reports change the colour", async () => {
	const s = await real("main");
	try {
		assert.deepEqual(s.border(), ["97"], "before any report");
		s.send(FOCUS_OUT);
		assert.deepEqual(s.border(), ["97"], "focus out");
		s.send(FOCUS_IN);
		assert.deepEqual(s.border(), ["94"], "focus in");
		// The same bytes must act the same in the fullscreen renderer.
		const full = await real("fullscreen");
		try {
			full.send(FOCUS_OUT);
			assert.deepEqual(full.border(), ["97"], "fullscreen focus out");
		} finally { full.shutdown(); }
	} finally { s.shutdown(); }
});

test("the fullscreen mode still handles the report itself and the editor never receives it as text", async () => {
	const s = await real("fullscreen");
	try {
		const viewport = s.tui as unknown as { selectionPressActive: boolean };
		viewport.selectionPressActive = true;
		s.send(FOCUS_OUT);
		assert.equal(viewport.selectionPressActive, false, "the fullscreen renderer ended its own selection press");
		assert.deepEqual(s.border(), ["97"], "and the box saw the report");
		s.send(FOCUS_IN);
		assert.equal(s.editor.getText(), "hi", "the editor's text is untouched");
		assert.ok(!strip(s.editor.render(30).join("\n")).includes("[O") && !strip(s.editor.render(30).join("\n")).includes("[I"), "no report in the rows");
		assert.deepEqual(s.border(), ["94"], "focus in seen too");
	} finally { s.shutdown(); }
});

test("session shutdown restores the fullscreen mode's own input handling", async () => {
	const s = await real("fullscreen");
	const tui = s.tui as unknown as { handleViewportInput: unknown; selectionPressActive: boolean };
	s.send(FOCUS_OUT);
	assert.deepEqual(s.border(), ["97"], "while bound the report reaches the box");
	s.shutdown();
	assert.ok(!Object.hasOwn(tui, "handleViewportInput"), "the instance carries no override after shutdown");
	assert.equal(tui.handleViewportInput, (TuiAltScreen.prototype as unknown as { handleViewportInput: unknown }).handleViewportInput, "pi-tui's own handler is back");
	tui.selectionPressActive = true;
	s.send(FOCUS_OUT);
	assert.equal(tui.selectionPressActive, false, "the fullscreen renderer still handles the report itself");
});

/** The editor's rows with the border/cursor of its first render after `act`. */
const cursorDrawn = (rows: string[]) => rows.some((row) => row.includes("\x1b_pi:fc\x07"));

test("in fullscreen mode a keystroke focuses a pane that reported nothing", async () => {
	const s = await real("fullscreen");
	try {
		assert.deepEqual(s.border(), ["97"], "before any input");
		s.send("x");
		const rows = s.editor.render(30);
		assert.ok(s.editor.getText().includes("x"), "the key was taken");
		assert.ok(cursorDrawn(rows), `the cursor cell is drawn: ${readable(rows.join("\n"))}`);
		assert.deepEqual(s.border(), ["94"], "the first render after the key");
	} finally { s.shutdown(); }
});

test("a keystroke focuses the pane again after a focus out report", async () => {
	const s = await real("fullscreen");
	try {
		s.send(FOCUS_IN);
		s.send(FOCUS_OUT);
		assert.deepEqual(s.border(), ["97"], "after focus out");
		s.send("x");
		assert.deepEqual(s.border(), ["94"], "after a key");
	} finally { s.shutdown(); }
});

test("in fullscreen mode a left button press on the input focuses a pane that reported nothing", async () => {
	const s = await real("fullscreen");
	try {
		assert.deepEqual(s.border(), ["97"], "before any input");
		s.send("\x1b[<0;5;2M");
		assert.deepEqual(s.border(), ["94"], "the first render after the press");
	} finally { s.shutdown(); }
});

test("in fullscreen mode a left button press above the input focuses a pane that reported nothing", async () => {
	const s = await real("fullscreen");
	try {
		const above = { render: () => ["above"], invalidate() {} };
		s.tui.children.unshift(above as never);
		assert.deepEqual(s.border(), ["97"], "before any input");
		s.send("\x1b[<0;5;1M");
		assert.deepEqual(s.border(), ["94"], "the first render after the press on the row above");
	} finally { s.shutdown(); }
});

test("in fullscreen mode a wheel turn, a mouse move, a button release and a right button press do not focus a pane that reported nothing", async () => {
	const s = await real("fullscreen");
	try {
		for (const data of ["\x1b[<64;5;2M", "\x1b[<35;5;2M", "\x1b[<0;5;2m", "\x1b[<2;5;2M"]) {
			s.send(data);
			assert.deepEqual(s.border(), ["97"], `after ${readable(data)}`);
		}
	} finally { s.shutdown(); }
});

test("a key that an extension shortcut takes focuses a pane that reported nothing", async () => {
	const s = await real("fullscreen");
	try {
		(s.editor as { onExtensionShortcut?: (data: string) => boolean }).onExtensionShortcut = () => true;
		assert.deepEqual(s.border(), ["97"], "before any input");
		s.send("\x1bb");
		const rows = s.editor.render(30);
		assert.ok(cursorDrawn(rows), `the cursor cell is drawn: ${readable(rows.join("\n"))}`);
		assert.deepEqual(s.border(), ["94"], "the first render after the shortcut key");
	} finally { s.shutdown(); }
});

/** The input widget: pi's Editor drawn as a rounded box in the Dango palette (contract 6.1.1, 6.9 "The input widget"). */
import "./pinned-themes.ts";
import { MOON_FRAMES } from "./loader-frames.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { visibleWidth, type AutocompleteProvider } from "@earendil-works/pi-tui";
import { CustomEditor } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/custom-editor.js";
import { WorkingStatusIndicator } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/status-indicator.js";
import { initTheme, theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

process.env.PI_CODING_AGENT_DIR ??= mkdtempSync(join(tmpdir(), "editor-box-"));
initTheme("dark", false);

const LOADER = MOON_FRAMES;
const FRAME = `[${LOADER.join("")}]`;
const LEFT_CAP = "\ue0b6";
const RIGHT_CAP = "\ue0b4";
const MARKER = "\x1b_pi:c\x07";
const BORDER = new Set(["╭", "╮", "╰", "╯", "─", "│"]);
const strip = (value: string) => value.replace(/\x1b\[[0-9;]*m/gu, "").replace(/\x1b_pi:\/?(?:fc|c)\x07/gu, "");
const readable = (value: string) => value.replace(/\x1b/gu, "\\e");

type Handler = (event: unknown, ctx?: unknown) => void;
type InputHandler = (data: string) => { consume?: boolean; data?: string } | undefined;

interface Session {
	editor: CustomEditor;
	renders: () => number;
	/** The terminal reports focus in (true) or out (false), through the path an extension sees. */
	focus(inside: boolean): void;
	/** The agent starts working: the extension's events, and pi's working status in the editor's border. */
	work(): void;
	dispose(): void;
}

/** The real CustomEditor with the compact-tools extension loaded as the other tests load it. */
async function session(options: { text?: string; rows?: number } = {}): Promise<Session> {
	const handlers = new Map<string, Handler[]>();
	const inputs: InputHandler[] = [];
	let renders = 0;
	let message: string | undefined;
	let indicator: { frames: string[]; intervalMs?: number } | undefined;
	const compactTools = (await import("../../compact-tools/extensions/compact-tools.ts")).default;
	compactTools({
		on: (name: string, handler: Handler) => handlers.set(name, [...(handlers.get(name) ?? []), handler]),
		registerTool: () => {},
		registerMarkdownTransformer: () => {},
		registerCommand: () => {},
		registerShortcut: () => {},
		getThinkingLevel: () => "medium",
	} as unknown as ExtensionAPI);
	const tui = { terminal: { rows: options.rows ?? 40, columns: 80 }, requestRender: () => { renders++; } };
	const noop = () => () => {};
	const ui = new Proxy({} as Record<string, unknown>, {
		get: (_target, key) => {
			if (key === "theme") return theme;
			if (key === "onTerminalInput") return (handler: InputHandler) => { inputs.push(handler); return () => {}; };
			if (key === "setWorkingMessage") return (value?: string) => { message = value; };
			if (key === "setWorkingIndicator") return (value?: typeof indicator) => { indicator = value; };
			if (key === "setWidget") return (_key: string, factory: unknown) => { if (typeof factory === "function") factory(tui); };
			return noop;
		},
	});
	const ctx = { cwd: "/project", mode: "tui", isProjectTrusted: () => false, isIdle: () => true, ui } as unknown as ExtensionContext;
	const emit = (name: string, event: unknown) => { for (const handler of handlers.get(name) ?? []) handler(event, ctx); };
	emit("session_start", { reason: "startup" });
	const keybindings = { matches: () => false } as never;
	const editor = new CustomEditor(tui as never, { borderColor: (text: string) => text, selectList: { selectedPrefix: (t: string) => t, selectedText: (t: string) => t, description: (t: string) => t, scrollInfo: (t: string) => t, noMatch: (t: string) => t } as never }, keybindings, { embedWorkingStatus: true });
	// Pi paints the border by thinking level at every change; the box must not obey it.
	editor.borderColor = (text: string) => theme.getThinkingBorderColor("high")(text);
	editor.focused = false;
	if (options.text !== undefined) editor.setText(options.text);
	let status: WorkingStatusIndicator | undefined;
	return {
		editor,
		renders: () => renders,
		focus(inside) {
			for (const handler of inputs) handler(inside ? "\x1b[I" : "\x1b[O");
		},
		work() {
			emit("agent_start", { type: "agent_start" });
			emit("message_update", { type: "message_update", message: { role: "assistant", content: [] }, assistantMessageEvent: { type: "toolcall_delta" } });
			assert.ok(message, "the extension sets a working message");
			status = new WorkingStatusIndicator(tui as never, message, indicator as never);
			editor.setWorkingStatusIndicator(status);
		},
		dispose() {
			status?.dispose();
			emit("agent_end", { type: "agent_end" });
			emit("session_shutdown", { reason: "quit" });
		},
	};
}

/** The foreground (SGR param) each cell of a row is drawn in; undefined is the default colour. */
function cells(row: string): Array<{ ch: string; fg: string | undefined }> {
	const out: Array<{ ch: string; fg: string | undefined }> = [];
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
		for (const ch of part) out.push({ ch, fg });
	}
	return out;
}

/** Every box-drawing cell of every row, with its colour. */
const borderFgs = (rows: string[]) => new Set(rows.flatMap((row) => cells(row).filter((cell) => BORDER.has(cell.ch)).map((cell) => cell.fg)));
const withBorder = (width: number, lines: string[]) => [
	`╭${"─".repeat(width - 2)}╮`,
	...lines.map((line) => `│ ${line.padEnd(width - 4)} │`),
	`╰${"─".repeat(width - 2)}╯`,
];

test("the editor draws a rounded box with no margin column and one padding column", async () => {
	const s = await session({ text: "fix the build\nthen ship it" });
	try {
		const rows = s.editor.render(40);
		assert.deepEqual(rows.map(strip), withBorder(40, ["fix the build", "then ship it"]), readable(rows.join("\n")));
	} finally { s.dispose(); }
});

test("every row of the editor is as wide as the pane", async () => {
	const s = await session({ text: "one\ntwo\nthree" });
	try {
		for (const width of [12, 24, 40, 80]) {
			const rows = s.editor.render(width).map(strip);
			assert.equal(rows.length, 5, `width ${width}: top, three text rows, bottom`);
			rows.forEach((row) => assert.equal(visibleWidth(row), width, `width ${width}: ${JSON.stringify(row)}`));
			assert.ok(rows[0]!.startsWith("╭") && rows[0]!.endsWith("╮"));
			assert.ok(rows.at(-1)!.startsWith("╰") && rows.at(-1)!.endsWith("╯"));
		}
	} finally { s.dispose(); }
});

test("typed text, wrapping and the cursor stay inside the box", async () => {
	const s = await session();
	try {
		s.editor.focused = true;
		s.editor.handleInput("a");
		s.editor.handleInput("b");
		assert.equal(s.editor.getText(), "ab");
		let rows = s.editor.render(20);
		assert.equal(strip(rows[1]!), `│ ${"ab ".padEnd(16)} │`, "the fake cursor is one cell after the text");
		// The cursor (the fake one, a reversed cell) sits after "ab" in column 4, inside the box.
		assert.ok(rows[1]!.includes(`ab${MARKER}`), "the cursor marker follows the text");
		// Text wraps at the text area, W-4 = 16 columns, and the walls stay put.
		s.editor.setText("x".repeat(40));
		rows = s.editor.render(20).map(strip);
		assert.equal(rows.length, 5, "three wrapped rows of at most 16 columns");
		rows.slice(1, -1).forEach((row) => {
			assert.equal(visibleWidth(row), 20);
			assert.ok(row.startsWith("│ ") && row.endsWith(" │"), JSON.stringify(row));
		});
		assert.ok(rows.slice(1, -1).every((row) => row.slice(2, -2).trimEnd().length <= 16));
	} finally { s.dispose(); }
});

test("the scroll labels stay on the borders of the box", async () => {
	const s = await session({ text: Array.from({ length: 30 }, (_, i) => `line ${i + 1}`).join("\n"), rows: 20 });
	try {
		// The cursor is at the end, so the view is scrolled down: the label is in the top border.
		let rows = s.editor.render(40).map(strip);
		assert.match(rows[0]!, /^╭─+ ↑ \d+ more ─+╮$/u, rows[0]);
		for (let i = 0; i < 30; i++) s.editor.handleInput("\x1b[A");
		rows = s.editor.render(40).map(strip);
		assert.match(rows.at(-1)!, /^╰─+ ↓ \d+ more ─+╯$/u, rows.at(-1));
		assert.ok(rows.every((row) => visibleWidth(row) === 40));
		assert.ok(rows.slice(1, -1).every((row) => row.startsWith("│ ") && row.endsWith(" │")));
	} finally { s.dispose(); }
});

test("a click in the box places the cursor at the clicked text column", async () => {
	const s = await session({ text: "hello world" });
	try {
		s.editor.render(40);
		// Column 2 is the first text column (0-based), so x = 2 + 4 is the fifth character.
		const click = (x: number) => s.editor.handleMouse({ type: "click", button: "left", x, y: 1, width: 40, height: 3 } as never);
		click(2 + 4);
		assert.deepEqual(s.editor.getCursor(), { line: 0, col: 4 });
		click(2);
		assert.deepEqual(s.editor.getCursor(), { line: 0, col: 0 });
		click(2 + 8);
		assert.deepEqual(s.editor.getCursor(), { line: 0, col: 8 });
	} finally { s.dispose(); }
});

test("the autocomplete rows start at the text column under the box", async () => {
	const s = await session();
	try {
		const provider: AutocompleteProvider = {
			triggerCharacters: ["/"],
			getSuggestions: async () => ({ items: [{ value: "/build", label: "build" }, { value: "/bench", label: "bench" }], prefix: "/" }),
			applyCompletion: (_lines: string[], line: number, col: number, item: { value: string }, prefix: string) => ({ lines: [item.value], cursorLine: line, cursorCol: item.value.length + col - prefix.length }),
		} as never;
		s.editor.setAutocompleteProvider(provider);
		s.editor.focused = true;
		s.editor.handleInput("/");
		await new Promise((resolve) => setTimeout(resolve, 100));
		const rows = s.editor.render(40).map(strip);
		assert.equal(rows.length > 3, true, "autocomplete rows follow the bottom border");
		assert.match(rows[2]!, /^╰─+╯$/u);
		const list = rows.slice(3);
		assert.ok(list.some((row) => row.includes("build")), JSON.stringify(list));
		for (const row of list) {
			assert.ok(/^ {2}(→ | {2})\S/u.test(row), `the list starts at column 2: ${JSON.stringify(row)}`);
			assert.ok(visibleWidth(row) <= 40);
		}
	} finally { s.dispose(); }
});

test("the border is colour 15 when the pane has no focus", async () => {
	const s = await session({ text: "hi" });
	try {
		s.focus(false);
		const rows = s.editor.render(30);
		assert.deepEqual([...borderFgs(rows)], ["97"], readable(rows.join("\n")));
		assert.ok(rows.join("").includes("\x1b[97m") && rows.join("").includes("\x1b[39m"));
	} finally { s.dispose(); }
});

test("the border is colour 15 before any focus report and colour 12 after focus in", async () => {
	const s = await session({ text: "hi" });
	try {
		assert.deepEqual([...borderFgs(s.editor.render(30))], ["97"], "before any report");
		s.focus(false);
		s.focus(true);
		assert.deepEqual([...borderFgs(s.editor.render(30))], ["94"], "after focus in");
	} finally { s.dispose(); }
});

test("the border is colour 6 while the agent works whatever the focus", async () => {
	const s = await session({ text: "hi" });
	try {
		s.work();
		for (const inside of [true, false]) {
			s.focus(inside);
			const rows = s.editor.render(50);
			assert.deepEqual([...borderFgs(rows)], ["36"], `focus ${inside}: ${readable(rows.join("\n"))}`);
			const top = strip(rows[0]!);
			assert.match(top, new RegExp(`^╭─${LEFT_CAP}${FRAME} Preparing tools…${RIGHT_CAP}─+╮$`, "u"), top);
			assert.equal(visibleWidth(top), 50);
			// The pill is one colour block: caps 6 and text inverted (ESC[7m) with the frame inside it, nothing of its own.
			const pill = rows[0]!.slice(rows[0]!.indexOf(LEFT_CAP));
			assert.ok(pill.includes("\x1b[7m") && pill.includes("\x1b[27m") && !pill.includes("\x1b[46m") && !pill.includes("\x1b[30m"), readable(pill));
			assert.match(rows[0]!, new RegExp(`\x1b\\[36m${LEFT_CAP}`, "u"));
			assert.match(rows[0]!, new RegExp(`\x1b\\[36m${RIGHT_CAP}`, "u"));
		}
		// A pane too narrow for the label truncates the words with … inside the pill and keeps the corner.
		const narrow = strip(s.editor.render(20).at(0)!);
		assert.equal(visibleWidth(narrow), 20);
		assert.match(narrow, new RegExp(`^╭─${LEFT_CAP}${FRAME} [^…]*…${RIGHT_CAP}─*╮$`, "u"), narrow);
	} finally { s.dispose(); }
});

test("the thinking level and bash mode do not colour the border", async () => {
	const s = await session({ text: "hi" });
	try {
		const seen = new Set<string>();
		for (const level of ["off", "minimal", "low", "medium", "high", "xhigh"] as const) {
			s.editor.borderColor = (text: string) => theme.getThinkingBorderColor(level)(text);
			for (const fg of borderFgs(s.editor.render(30))) seen.add(String(fg));
		}
		s.editor.borderColor = (text: string) => theme.getBashModeBorderColor()(text);
		for (const fg of borderFgs(s.editor.render(30))) seen.add(String(fg));
		assert.deepEqual([...seen], ["97"], "one colour whatever pi assigns to borderColor");
		// Typed text keeps the default colour.
		const text = cells(s.editor.render(30)[1]!).filter((cell) => /[hi]/u.test(cell.ch));
		assert.ok(text.length >= 2 && text.every((cell) => cell.fg === undefined));
	} finally { s.dispose(); }
});

test("a focus report changes the colour and asks for a render", async () => {
	const s = await session({ text: "hi" });
	try {
		s.focus(true);
		assert.deepEqual([...borderFgs(s.editor.render(30))], ["94"], "focus in");
		const before = s.renders();
		s.focus(false);
		assert.ok(s.renders() > before, "focus out asks for a render");
		assert.deepEqual([...borderFgs(s.editor.render(30))], ["97"]);
		const mid = s.renders();
		s.focus(true);
		assert.ok(s.renders() > mid, "focus in asks for a render");
		assert.deepEqual([...borderFgs(s.editor.render(30))], ["94"]);
	} finally { s.dispose(); }
});

test("the box draws indexed SGR only", async () => {
	const s = await session({ text: "fix the build\nthen ship it" });
	try {
		const all: string[] = [];
		const collect = () => all.push(...s.editor.render(40));
		collect();
		s.focus(false);
		collect();
		s.work();
		collect();
		const joined = all.join("");
		assert.ok(joined.includes("╭") && joined.includes("│"), "the box is drawn");
		for (const [, params] of joined.matchAll(/\x1b\[([0-9;]*)m/gu)) {
			const codes = params!.split(";");
			assert.ok(!codes.includes("2") && !codes.includes("7") || params === "7", `no faint SGR: ${params}`);
			assert.ok(!/^(38|48);2/u.test(params!), `no RGB: ${params}`);
			if (/^(38|48);5/u.test(params!)) assert.ok(Number(codes[2]) < 16, `index < 16: ${params}`);
		}
		assert.ok(!/#[0-9a-f]{6}/iu.test(joined));
		// The box's own cells use the palette's indices; the one reverse-video cell is the cursor, not the box.
		assert.ok(borderFgs(all.slice(0, 4)).size === 1);
		assert.ok([...borderFgs(all)].every((fg) => ["94", "97", "36"].includes(String(fg))), readable([...borderFgs(all)].join()));
	} finally { s.dispose(); }
});

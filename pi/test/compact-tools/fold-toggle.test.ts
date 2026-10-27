/**
 * fold: "toggle", the fold language, as docs/contract.md section 6.9 fixes it (the fork's tests, moved here).
 * cpd's defaults are fold "toggle" with the cockpit's glyphs; the fork's were "status" and ▸ ▾, which are
 * the base of the merge tests below and what `STATUS` draws. The defaults themselves, with no config file,
 * are pinned by defaults.test.ts. Glyph expectations are code points, never pasted glyphs.
 */
import assert from "node:assert/strict";
import test from "node:test";
import {
	advance, announce, CASES, CUSTOM_CASES, loadExtension, makeRow, restoreClocks, setNow, shutdown, type Harness,
} from "./indicator-harness.ts";
import { DEFAULT_CONFIG, mergeConfig } from "../../compact-tools/extensions/compact-tools-config.ts";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";

// The machine-local time is the process timezone's: pin it, so `03:11` is the same on every machine.
process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");

const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");
const BOX = { head: "\u256D", close: "\u2570", rail: "\u2502", plainClose: "\u2514" }; // ╭ ╰ │ └
const PLAIN = { expand: "\u25B8", collapse: "\u25BE" }; // ▸ ▾
const NERD = { expand: "\u{F0B26}", collapse: "\u{F0B2C}" };
const NERD_JSON = { expand: "\uDB82\uDF26", collapse: "\uDB82\uDF2C" }; // surrogate pairs, as a config file spells them

type Folded = { fold?: unknown; toggle?: unknown };
const foldOf = (config: object) => config as unknown as Folded;

/** The base of the merge tests: the fork's own defaults, `fold: "status"` and the plain glyphs. */
const FORK_BASE = { ...DEFAULT_CONFIG, ...{ fold: "status", toggle: PLAIN } } as typeof DEFAULT_CONFIG;

/** mergeConfig's warnings, which go to console.error. */
function merged(value: unknown, base = FORK_BASE) {
	const warnings: string[] = [];
	const original = console.error;
	console.error = (...args: unknown[]) => { warnings.push(args.join(" ")); };
	try { return { config: mergeConfig(base, value, "test"), warnings }; } finally { console.error = original; }
}

// --- config ---------------------------------------------------------------------------------

test("fold defaults to toggle and the glyphs to the cockpit's, and a file without them keeps those", () => {
	assert.equal(foldOf(DEFAULT_CONFIG).fold, "toggle");
	assert.deepEqual(foldOf(DEFAULT_CONFIG).toggle, NERD);
	const { config, warnings } = merged({}, DEFAULT_CONFIG);
	assert.equal(foldOf(config).fold, "toggle");
	assert.deepEqual(foldOf(config).toggle, NERD);
	assert.deepEqual(warnings, []);
});

test("fold accepts toggle and status, spelled exactly", () => {
	assert.equal(foldOf(merged({ fold: "toggle" }).config).fold, "toggle");
	assert.equal(foldOf(merged({ fold: "status" }, merged({ fold: "toggle" }).config).config).fold, "status");
});

test("an unknown fold warns and keeps the previous value, as the loader does for style", () => {
	for (const bad of ["Toggle", "fancy", "", true, 1, null, ["toggle"]]) {
		const { config, warnings } = merged({ fold: bad });
		assert.equal(foldOf(config).fold, "status", JSON.stringify(bad));
		assert.equal(warnings.length, 1, JSON.stringify(bad));
		assert.match(warnings[0]!, /fold/u);
	}
	const { config } = merged({ fold: "fancy" }, merged({ fold: "toggle" }).config);
	assert.equal(foldOf(config).fold, "toggle", "the previous value, not the default");
});

test("toggle overrides the glyphs field by field, code points and JSON escapes alike", () => {
	assert.deepEqual(foldOf(merged({ toggle: NERD }).config).toggle, NERD);
	assert.deepEqual(foldOf(merged({ toggle: { expand: "+" } }).config).toggle, { expand: "+", collapse: PLAIN.collapse });
	assert.deepEqual(foldOf(merged({ toggle: { collapse: "-" } }).config).toggle, { expand: PLAIN.expand, collapse: "-" });
	// A config file is JSON.parse'd, which turns the escapes into the code points.
	const parsed = JSON.parse(`{"toggle":{"expand":"\\uDB82\\uDF26","collapse":"\\uDB82\\uDF2C"}}`);
	assert.deepEqual(parsed.toggle, NERD_JSON);
	assert.deepEqual(foldOf(merged(parsed).config).toggle, NERD);
	assert.equal([...NERD.expand][0]!.codePointAt(0), 0xF0B26);
});

test("an invalid glyph warns and keeps the previous glyph", () => {
	for (const bad of ["", 7, null, "a\nb", "a\x1bb", "\x00"]) {
		const { config, warnings } = merged({ toggle: { expand: bad, collapse: "-" } });
		assert.deepEqual(foldOf(config).toggle, { expand: PLAIN.expand, collapse: "-" }, JSON.stringify(bad));
		assert.equal(warnings.length, 1, JSON.stringify(bad));
		assert.match(warnings[0]!, /toggle\.expand/u);
	}
});

test("a toggle that is not an object warns and keeps the previous glyphs", () => {
	for (const bad of ["▸", 1, ["a", "b"], null]) {
		const { config, warnings } = merged({ toggle: bad });
		assert.deepEqual(foldOf(config).toggle, PLAIN, JSON.stringify(bad));
		assert.equal(warnings.length, 1, JSON.stringify(bad));
		assert.match(warnings[0]!, /toggle/u);
	}
});

test("project settings override fold and each glyph field by field", () => {
	const global = merged({ fold: "toggle", toggle: NERD }).config;
	const project = merged({ toggle: { collapse: "-" } }, global).config;
	assert.equal(foldOf(project).fold, "toggle");
	assert.deepEqual(foldOf(project).toggle, { expand: NERD.expand, collapse: "-" });
});

test("the other keys are unaffected by fold and toggle", () => {
	const { config } = merged({ fold: "toggle", toggle: NERD });
	assert.equal(foldOf(config).fold, "toggle");
	const { fold: _f, toggle: _t, ...rest } = config as unknown as Record<string, unknown>;
	const { fold: _df, toggle: _dt, ...defaults } = DEFAULT_CONFIG as unknown as Record<string, unknown>;
	assert.deepEqual(rest, defaults);
});

// --- compact rows -----------------------------------------------------------------------------

let sequence = 0;
const WIDTH = 100;

interface Drawn { head: string[]; running: string[]; done: string[] }

/** A row run to its end in Pi's order, drawn while it runs and once it has its result. */
async function drawRow(config: object, name: string, options: { failed?: boolean; expanded?: boolean } = {}): Promise<Drawn> {
	const harness: Harness = await loadExtension({ style: "compact", ...config }, { defaults: true });
	try {
		const id = `fold-${++sequence}`;
		const outcome = (CASES[name] ?? CUSTOM_CASES[name])!;
		announce(harness, id, name);
		const row = makeRow(name, id, outcome.args, harness.definitions.get(name));
		row.setExpanded(options.expanded ?? false);
		row.setArgsComplete();
		setNow(TIME_MS);
		harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: name, args: outcome.args });
		row.markExecutionStarted();
		const running = row.render(WIDTH).map(plain);
		advance(300);
		harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: options.failed ?? false });
		row.updateResult({ ...(options.failed ? outcome.failure : outcome.success), isError: options.failed ?? false } as never, false);
		const done = row.render(WIDTH).map(plain);
		return { head: done, running, done };
	} finally {
		shutdown(harness);
	}
}

const TOGGLE = { fold: "toggle" };
const STATUS = { fold: "status" };
const lastLine = (lines: string[]) => lines.filter((line) => line.trim() !== "").at(-1)!;
const firstLine = (lines: string[]) => lines.find((line) => line.trim() !== "")!;
/** What follows the ` └ ` of a status-fold closing row: the row's metadata. */
const metadataOf = (statusLines: string[]) => {
	const line = lastLine(statusLines);
	assert.ok(line.startsWith(` ${BOX.plainClose} `), line);
	return line.slice(` ${BOX.plainClose} `.length);
};

test("the toggle head is '╭─' and the pill, with no dot, where status keeps its dot head", async () => {
	for (const name of ["bash", "read", "web_search"]) {
		const status = await drawRow(STATUS, name);
		const toggle = await drawRow(TOGGLE, name);
		assert.ok(firstLine(status.done).startsWith(" "), name);
		assert.ok(firstLine(toggle.done).startsWith(`${BOX.head}\u2500`), `${name}: ${firstLine(toggle.done)}`);
		assert.ok(!firstLine(toggle.done).includes("\u2981"), `${name}: no dot`);
	}
});

test("a folded row closes with ╰<glyph> expand · <metadata>", async () => {
	for (const name of ["bash", "read", "grep", "find", "ls", "web_search"]) {
		const status = await drawRow(STATUS, name);
		const toggle = await drawRow({ ...TOGGLE, toggle: NERD }, name);
		assert.equal(lastLine(toggle.done), `${BOX.close}${NERD.expand} expand · 03:11 · ${metadataOf(status.done)}`, name);
		assert.equal(toggle.done.filter((line) => line.includes("└")).length, 0, `${name}: no └ left`);
	}
});

test("an expanded row closes with ╰<glyph> collapse · <metadata> under its '│ ' body", async () => {
	for (const name of ["bash", "read", "web_search"]) {
		const status = await drawRow(STATUS, name, { expanded: true });
		const toggle = await drawRow({ ...TOGGLE, toggle: NERD }, name, { expanded: true });
		assert.equal(lastLine(toggle.done), `${BOX.close}${NERD.collapse} collapse · 03:11 · ${metadataOf(status.done)}`, name);
		// The same body rows, the toggle's with no margin column before the rail.
		const body = (lines: string[], rail: string) => lines.filter((line) => line.startsWith(rail)).map((line) => line.slice(rail.length - 2));
		assert.deepEqual(body(toggle.done, `${BOX.rail} `), body(status.done, ` ${BOX.rail} `), `${name}: the body rows are unchanged`);
		assert.ok(body(toggle.done, `${BOX.rail} `).length > 0, name);
	}
});

test("the default glyphs are U+F0B26 and U+F0B2C", async () => {
	const folded = await drawRow(TOGGLE, "read");
	assert.match(lastLine(folded.done), new RegExp(`^${BOX.close}${NERD.expand} expand · 03:11 · Done in `, "u"));
	const expanded = await drawRow(TOGGLE, "read", { expanded: true });
	assert.match(lastLine(expanded.done), new RegExp(`^${BOX.close}${NERD.collapse} collapse · 03:11 · Done in `, "u"));
	const plain = await drawRow({ ...TOGGLE, toggle: PLAIN }, "read");
	assert.match(lastLine(plain.done), new RegExp(`^${BOX.close}${PLAIN.expand} expand · 03:11 · Done in `, "u"), "a file can still ask for ▸ and ▾");
});

test("a row with no metadata closes with the toggle and the time alone, or the bare toggle when too narrow", async () => {
	// A custom tool with an empty result has no line count; a restored row has no live timing, so
	// only its branch entry can give the time, and the closing row is the toggle and the time.
	const id = `fold-${++sequence}`;
	const branch = [{ type: "message", timestamp: new Date(TIME_MS).toISOString(), message: { role: "toolResult", toolName: "web_search", toolCallId: id, timestamp: TIME_MS } }];
	const harness = await loadExtension({ style: "compact", ...TOGGLE, toggle: NERD }, { defaults: true, sessionManager: { getBranch: () => branch } });
	try {
		const row = makeRow("web_search", id, CUSTOM_CASES.web_search!.args, undefined);
		row.setArgsComplete();
		row.updateResult({ content: [], details: undefined, isError: false } as never, false);
		const lines = row.render(WIDTH).map(plain);
		const closing = lastLine(lines);
		assert.equal(closing, `${BOX.close}${NERD.expand} expand · 03:11`);
		// One column short of the toggle and time: the time is omitted and the row is unchanged.
		assert.equal(lastLine(row.render(19).map(plain)), `${BOX.close}${NERD.expand} expand · 03:11`);
		assert.equal(lastLine(row.render(18).map(plain)), `${BOX.close}${NERD.expand} expand`);
	} finally { shutdown(harness); }
});

test("a failed row closes with ╰<glyph> expand · Failed in …", async () => {
	for (const expanded of [false, true]) {
		const status = await drawRow(STATUS, "bash", { failed: true, expanded });
		const toggle = await drawRow({ ...TOGGLE, toggle: NERD }, "bash", { failed: true, expanded });
		const word = expanded ? "collapse" : "expand";
		const glyph = expanded ? NERD.collapse : NERD.expand;
		const closing = lastLine(toggle.done);
		assert.equal(closing, `${BOX.close}${glyph} ${word} · 03:11 · ${metadataOf(status.done)}`);
		assert.match(closing, /· 03:11 · Failed in 0\.300s/u);
	}
	const folded = await drawRow(TOGGLE, "bash", { failed: true });
	assert.match(lastLine(folded.done), /· 03:11 · Failed in 0\.300s · Command exited with code 1$/u, "with a reason, after the time");
});

test("an edit's closing row carries its diff counts after the toggle", async () => {
	const toggle = await drawRow({ ...TOGGLE, toggle: NERD }, "edit");
	assert.match(lastLine(toggle.done), new RegExp(`^${BOX.close}${NERD.expand} expand · 03:11 · Done in 0\\.300s \\(\\+1 -1\\)$`, "u"));
});

test("while a row runs the head wears '╭─' with no dot and the closing row is the loader, with no toggle", async () => {
	const toggle = await drawRow(TOGGLE, "bash");
	assert.ok(firstLine(toggle.running).startsWith(`${BOX.head}\u2500`), firstLine(toggle.running));
	assert.ok(!firstLine(toggle.running).includes("\u2981"), "no dot");
	assert.ok(toggle.running.every((line) => !line.includes("expand") && !line.includes("collapse")), "no toggle while running");
	assert.equal(toggle.running.filter((line) => line.includes(BOX.close)).length, 1, "the loader's ╰ while running");
});

test("fold: toggle only acts in the compact style", async () => {
	assert.equal(foldOf(merged(TOGGLE).config).fold, "toggle", "the key is accepted");
	for (const style of ["claude", "codex"]) {
		const without = await drawRow({ style, ...STATUS }, "bash");
		const withToggle = await drawRow({ style, ...TOGGLE }, "bash");
		assert.deepEqual(withToggle.done, without.done, style);
		assert.ok(withToggle.done.every((line) => !line.includes(BOX.close) && !line.includes(BOX.head)), style);
	}
});

test("with no fold key every compact row is the toggle's; fold: status brings back upstream's rows", async () => {
	assert.equal(foldOf(merged({}, DEFAULT_CONFIG).config).fold, "toggle", "the key's default is toggle");
	for (const name of ["bash", "read", "edit", "web_search"]) {
		for (const expanded of [false, true]) {
			const absent = await drawRow({}, name, { expanded });
			// The running frame carries the loader, which moves with the clock: the finished row is the same.
			assert.deepEqual(absent.done, (await drawRow(TOGGLE, name, { expanded })).done, name);
			assert.ok(firstLine(absent.done).startsWith(BOX.head), name);
			const status = await drawRow(STATUS, name, { expanded });
			assert.ok(firstLine(status.done).startsWith(" "), name);
			assert.match(lastLine(status.done), /^ └ (Done in [0-9.]+s|Failed in )|^ │ /u, name);
			assert.ok(status.done.every((line) => !line.includes(BOX.close) && !line.includes(BOX.head)), name);
		}
	}
});

// --- thinking block ---------------------------------------------------------------------------

/** The thinking block as the extension's Markdown transformer draws it, and Ctrl+T as the user presses it. */
async function thinking(config: object, source = "## **Check the implementation**\n\nInspect the renderer.\nKeep the cache.") {
	await loadExtension({ style: "compact", ...config }, { defaults: true });
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
	const draw = () => transforms.reduce((value, fn) => fn(value, { messageType: "assistant-thinking", isStreaming: false, availableWidth: 80 }), source)
		.split("\n").map(plain);
	return {
		draw,
		press: () => input?.("\x14"),
		end: () => handlers.get("session_shutdown")?.forEach((fn) => fn({ reason: "quit" }, ctx)),
	};
}

const THINKING_TITLE = "╭─\uE0B6Thinking…\uE0B4";

test("thinking, folded (summary): the one Thinking pill, then ╰<glyph> expand", async () => {
	const block = await thinking({ ...TOGGLE, toggle: NERD });
	try {
		assert.deepEqual(block.draw().map((line) => line.trim()), [THINKING_TITLE, `${BOX.close}${NERD.expand} expand`]);
	} finally { block.end(); }
});

test("thinking, expanded (detail): the whole text on │ lines, then ╰<glyph> collapse", async () => {
	const block = await thinking({ ...TOGGLE, toggle: NERD });
	try {
		block.draw(); // the controller learns the block has detail on its first draw, as in Pi
		block.press();
		assert.deepEqual(block.draw().map((line) => line.trim()), [
			THINKING_TITLE,
			`${BOX.rail} ## **Check the implementation**`,
			BOX.rail,
			`${BOX.rail} Inspect the renderer.`,
			`${BOX.rail} Keep the cache.`,
			`${BOX.close}${NERD.collapse} collapse`,
		]);
	} finally { block.end(); }
});

test("thinking: summary after detail is folded again, hidden is unchanged, the default glyphs are the cockpit's", async () => {
	const block = await thinking(TOGGLE);
	try {
		assert.equal(block.draw().at(-1)!.trim(), `${BOX.close}${NERD.expand} expand`);
		block.press();
		assert.equal(block.draw().at(-1)!.trim(), `${BOX.close}${NERD.collapse} collapse`);
		block.press();
		assert.deepEqual(block.draw().map((line) => line.trim()), [THINKING_TITLE, `${BOX.close}${NERD.expand} expand`]);
		assert.equal(block.press(), undefined, "summary-after-detail passes to Pi's own toggle, which hides");
		block.press();
	} finally { block.end(); }
});

test("thinking of one paragraph has its toggle too: the whole text is inside the fold", async () => {
	assert.equal(foldOf(merged(TOGGLE).config).fold, "toggle", "the key is accepted");
	const block = await thinking(TOGGLE, "**Planning integration harness testing**");
	try {
		assert.deepEqual(block.draw().map((line) => line.trim()), [THINKING_TITLE, `${BOX.close}${NERD.expand} expand`]);
	} finally { block.end(); }
});

test("with fold: status the thinking block is upstream's: └ ctrl+t toggle • click to hide", async () => {
	assert.equal(foldOf(merged({}).config).fold, "status", "the fork's base is status");
	for (const config of [STATUS]) {
		const block = await thinking(config);
		try {
			assert.deepEqual(block.draw().map((line) => line.trimEnd()), ["Thinking…"]);
			block.press();
			assert.deepEqual(block.draw().map((line) => line.trimEnd()), [
				"\uE0B6Thinking…\uE0B4",
				`${BOX.rail} ## **Check the implementation**`,
				BOX.rail,
				`${BOX.rail} Inspect the renderer.`,
				`${BOX.rail} Keep the cache.`,
				`${BOX.plainClose} ctrl+t toggle • click to hide`,
			]);
		} finally { block.end(); }
	}
});

test.after(() => restoreClocks());

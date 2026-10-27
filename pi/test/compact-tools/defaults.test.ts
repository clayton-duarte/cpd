/**
 * The extension's defaults are the fold language, in code, with no compact-tools.json (docs/contract.md section 6.9,
 * "The defaults"). Everything here runs with no config file: fold-toggle.test.ts pins the shapes with `fold`
 * and `toggle` given in a file, this file what an install that has no file at all draws.
 * Glyph expectations are code points, never pasted glyphs.
 */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import type { ExtensionContext } from "@earendil-works/pi-coding-agent";
import { DEFAULT_CONFIG, loadConfig } from "../../compact-tools/extensions/compact-tools-config.ts";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

// The agent directory of this file, pi's getAgentDir(). The harness points PI_CODING_AGENT_DIR at a directory of its own when it
// loads, so it is loaded when a test needs it (tests of the config alone do not), and the variable set back.
const AGENT_DIR = mkdtempSync(join(tmpdir(), "compact-tools-agent-"));
process.env.PI_CODING_AGENT_DIR = AGENT_DIR;
let harnessLoaded = false;
async function loadHarness() {
	const loaded = await import("./indicator-harness.ts");
	harnessLoaded = true;
	process.env.PI_CODING_AGENT_DIR = AGENT_DIR;
	return loaded;
}

// The machine-local time is the process timezone's: pin it, so `03:11` is the same on every machine.
process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");

const EXPAND = "\u{F0B26}";
const COLLAPSE = "\u{F0B2C}";
const HEAD = "\u256D\u2500"; // "╭─"
const CLOSE = "\u2570"; // ╰
const SEVEN = ["read", "write", "edit", "bash", "grep", "find", "ls"];

const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");
const lastLine = (lines: string[]) => lines.filter((line) => line.trim() !== "").at(-1)!;
const firstLine = (lines: string[]) => lines.find((line) => line.trim() !== "")!;

/** An agent directory and a project with no compact-tools.json in either, as an install that never wrote one has. */
function noConfigFile<T>(fn: (cwd: string) => T): T {
	const cwd = mkdtempSync(join(tmpdir(), "compact-tools-defaults-"));
	rmSync(join(process.env.PI_CODING_AGENT_DIR!, "compact-tools.json"), { force: true });
	try { return fn(cwd); } finally { rmSync(cwd, { recursive: true, force: true }); }
}

test("DEFAULT_CONFIG is the fold language: toggle, U+F0B26, U+F0B2C, seven tools, style compact, the rest upstream's", () => {
	assert.equal(DEFAULT_CONFIG.style, "compact");
	assert.equal(DEFAULT_CONFIG.fold, "toggle");
	assert.deepEqual(DEFAULT_CONFIG.toggle, { expand: EXPAND, collapse: COLLAPSE });
	assert.equal([...DEFAULT_CONFIG.toggle.expand].length, 1, "one code point");
	assert.equal(DEFAULT_CONFIG.toggle.expand.codePointAt(0), 0xF0B26);
	assert.equal(DEFAULT_CONFIG.toggle.collapse.codePointAt(0), 0xF0B2C);
	assert.deepEqual(DEFAULT_CONFIG.tools, SEVEN);
	// The rest is upstream's.
	assert.equal(DEFAULT_CONFIG.mode, "normal");
	assert.equal(DEFAULT_CONFIG.previewLines, 10);
	assert.deepEqual(DEFAULT_CONFIG.custom_tools, { enabled: true, auto_compact: true, exclude: [] });
	for (const [tool, value] of Object.entries(DEFAULT_CONFIG.auto_compact)) assert.equal(value, tool !== "edit", `auto_compact.${tool}`);
});

test("with no compact-tools.json anywhere, loadConfig is DEFAULT_CONFIG, for a trusted project and an untrusted one", () => {
	noConfigFile((cwd) => {
		assert.deepEqual(loadConfig(cwd, true), DEFAULT_CONFIG);
		assert.deepEqual(loadConfig(cwd, false), DEFAULT_CONFIG);
		assert.equal(loadConfig(cwd, true).fold, "toggle");
	});
});

interface Drawn { running: string[]; done: string[] }

/** One of the tools run to its end on the extension's own defaults (no file), drawn while it runs and with its result. */
async function draw(name: string, expanded = false): Promise<Drawn> {
	const { advance, announce, CASES, loadExtension, makeRow, setNow, shutdown } = await loadHarness();
	const harness = await loadExtension({}, { defaults: true });
	try {
		const id = `defaults-${name}-${expanded}`;
		const outcome = CASES[name]!;
		announce(harness, id, name);
		const row = makeRow(name, id, outcome.args, harness.definitions.get(name));
		row.setExpanded(expanded);
		row.setArgsComplete();
		setNow(TIME_MS);
		harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: name, args: outcome.args });
		row.markExecutionStarted();
		const running = row.render(100).map(plain);
		advance(300);
		harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: false });
		row.updateResult({ ...outcome.success, isError: false } as never, false);
		return { running, done: row.render(100).map(plain) };
	} finally {
		shutdown(harness);
	}
}

test("with no file the extension registers the seven tools and no other", async () => {
	const { loadExtension, shutdown } = await loadHarness();
	const harness = await loadExtension({}, { defaults: true });
	try {
		assert.deepEqual([...harness.definitions.keys()].sort(), [...SEVEN].sort());
		assert.ok(harness.commands.has("compact-tools"), "the /compact-tools command is kept");
	} finally { shutdown(harness); }
});

test("with no file a compact row opens with ╭ and closes with ╰ U+F0B26 expand · Done in …, for each of the seven tools", async () => {
	for (const name of SEVEN) {
		const { done } = await draw(name);
		assert.ok(firstLine(done).startsWith(HEAD), `${name}: ${firstLine(done)}`);
		assert.match(lastLine(done), new RegExp(`^${CLOSE}${EXPAND} expand · 03:11 · Done in 0\\.300s`, "u"), name);
		assert.ok(done.every((line) => !line.includes("\u2514")), `${name}: no └ left`);
	}
});

test("with no file an expanded row closes with ╰ U+F0B2C collapse · Done in … under its │ body", async () => {
	for (const name of ["bash", "read", "edit"]) {
		const { done } = await draw(name, true);
		assert.match(lastLine(done), new RegExp(`^${CLOSE}${COLLAPSE} collapse · 03:11 · Done in 0\\.300s`, "u"), name);
		assert.ok(done.some((line) => line.startsWith("\u2502 ")), `${name}: the body rail`);
	}
});

test("with no file the row still opens with ╭ while it runs, and closes with the loader (rail-end.test.ts)", async () => {
	const { running } = await draw("bash");
	assert.ok(firstLine(running).startsWith(HEAD), firstLine(running));
	assert.equal(running.filter((line) => line.includes(CLOSE)).length, 1, "one ╰ while running: the loader");
});

/** The thinking block as the extension's Markdown transformer draws it, on the defaults, and Ctrl+T as the user presses it. */
async function thinking(source = "## **Check the implementation**\n\nInspect the renderer.\nKeep the cache.") {
	const { loadExtension } = await loadHarness();
	await loadExtension({}, { defaults: true });
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
	} as never);
	handlers.get("session_start")?.forEach((fn) => fn({ reason: "startup" }, ctx));
	return {
		draw: () => transforms.reduce((value, fn) => fn(value, { messageType: "assistant-thinking", isStreaming: false, availableWidth: 80 }), source)
			.split("\n").map((line) => plain(line).trimEnd()),
		press: () => input?.("\x14"),
		end: () => handlers.get("session_shutdown")?.forEach((fn) => fn({ reason: "quit" }, ctx)),
	};
}

test("with no file the thinking block's control row is the toggle: ╰ U+F0B26 expand, then ╰ U+F0B2C collapse", async () => {
	const block = await thinking();
	try {
		assert.deepEqual(block.draw(), ["╭─\uE0B6Thinking…\uE0B4", `${CLOSE}${EXPAND} expand`]);
		block.draw(); // the controller learns the block has detail on its first draw, as in Pi
		block.press();
		assert.deepEqual(block.draw(), [
			"╭─\uE0B6Thinking…\uE0B4",
			"\u2502 ## **Check the implementation**",
			"\u2502",
			"\u2502 Inspect the renderer.",
			"\u2502 Keep the cache.",
			`${CLOSE}${COLLAPSE} collapse`,
		]);
	} finally { block.end(); }
});

test.after(async () => {
	if (harnessLoaded) (await import("./indicator-harness.ts")).restoreClocks();
	rmSync(AGENT_DIR, { recursive: true, force: true });
});

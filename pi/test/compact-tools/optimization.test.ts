import "./pinned-themes.ts";
import { uiRenderers } from "./ui-renderers.ts";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { initTheme, ToolExecutionComponent, type ExtensionAPI, type ToolDefinition } from "@earendil-works/pi-coding-agent";
import { stripTerminalSequences } from "@earendil-works/pi-tui";
import compactTools from "../../compact-tools/extensions/compact-tools.ts";
import { loadConfig } from "../../compact-tools/extensions/compact-tools-config.ts";
import { renderCodeView } from "../../compact-tools/extensions/compact-tools-layout.ts";
import { ToolRuntime } from "../../compact-tools/extensions/compact-tools-runtime.ts";
import { findAnchorTop } from "../../compact-tools/extensions/compact-tools-viewport.ts";

/** Original allocation-heavy algorithm: differential tests protect candidate ordering and ties. */
function originalAnchor(before: string[], top: number, height: number, after: string[]): number | undefined {
	const starts = new Map<string, number[]>();
	after.forEach((line, index) => {
		const list = starts.get(line);
		if (list) list.push(index);
		else starts.set(line, [index]);
	});
	const bottom = Math.min(before.length, top + Math.max(1, height));
	const candidates: number[] = [];
	for (let row = top; row < bottom; row++) candidates.push(row);
	for (let row = top - 1; row >= 0; row--) candidates.push(row);
	for (const row of candidates) {
		const window = before.slice(row, row + 2);
		if (window.length < 2 || !window.some((line) => stripTerminalSequences(line).trim().length > 0)) continue;
		let best: number | undefined;
		let bestRun = 0;
		for (const start of starts.get(window[0]!) ?? []) {
			let run = 0;
			while (run < 200 && row + run < before.length && before[row + run] === after[start + run]) run++;
			if (run < 2) continue;
			if (best === undefined || run > bestRun || (run === bestRun && Math.abs(start - row) < Math.abs(best - row))) {
				best = start;
				bestRun = run;
			}
		}
		if (best !== undefined) return Math.max(0, best - (row - top));
	}
	return undefined;
}

test("allocation-free anchor selection matches the original over 2,000 seeded relayouts", () => {
	let seed = 123456789;
	const random = (maximum: number) => {
		seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
		return seed % maximum;
	};
	const values = ["", " ", "a", "b", "c", "한글 😀", "\x1b[31mtext\x1b[0m", "\x1b[0m"];
	for (let iteration = 0; iteration < 2_000; iteration++) {
		const before = Array.from({ length: random(60) }, () => values[random(values.length)]!);
		const after = [...before];
		after.splice(random(after.length + 1), random(20), ...Array.from({ length: random(10) }, () => values[random(values.length)]!));
		const top = random(before.length + 1);
		const height = random(30);
		assert.equal(findAnchorTop(before, top, height, after), originalAnchor(before, top, height, after), `relayout ${iteration}`);
	}
	assert.equal(findAnchorTop(["a", "b"], 100, 5, ["a", "b"]), originalAnchor(["a", "b"], 100, 5, ["a", "b"]));
});

test("code views are cached per width and draw every row", () => {
	initTheme("dark", false);
	const component = renderCodeView("one\ntwo\nthree", "NOTES", {} as Parameters<typeof renderCodeView>[2], { rail: "" })!;
	const first = component.render(80);
	assert.equal(component.render(80), first, "cached width must do no work");
	assert.deepEqual(component.render(40).map(stripTerminalSequences), [" 1 one", " 2 two", " 3 three"]);
});

test("150,000-line file views do not exceed the argument limit and retain six-digit gutters", () => {
	initTheme("dark", false);
	const component = renderCodeView(Array(150_000).fill("x").join("\n"), "large.ts", {} as Parameters<typeof renderCodeView>[2], { rail: "" })!;
	const rows = component.render(80);
	assert.equal(rows.length, 150_000);
	assert.equal(stripTerminalSequences(rows[0]!), "      1 x");
	assert.equal(stripTerminalSequences(rows.at(-1)!), " 150000 x");
	assert.equal(component.render(80), rows, "unchanged layout is cached");
});

for (const source of ["events", "rows"] as const) {
	test(`timing retention is bounded without TUI rendering (${source})`, () => {
		const runtime = new ToolRuntime();
		runtime.clearTimings();
		try {
			for (let index = 0; index < 2_100; index++) {
				const id = `${source}-${index}`;
				if (source === "events") {
					runtime.noteExecutionStart(id);
					runtime.noteExecutionEnd(id);
				} else runtime.syncRow({ toolCallId: id, state: {}, executionStarted: true } as Parameters<ToolRuntime["syncRow"]>[0], true);
			}
			const timings = (globalThis as Record<symbol, Map<string, unknown>>)[Symbol.for("pi.compact-tools.execution-timings")]!;
			assert.equal(timings.size, 2_000);
			assert.equal(runtime.timing(`${source}-0`), undefined);
			assert.ok(runtime.timing(`${source}-2099`)?.endedAt);
		} finally { runtime.reset(true); }
	});
}

test("timing eviction preserves active siblings and bounds an all-active workload", () => {
	const runtime = new ToolRuntime();
	runtime.clearTimings();
	try {
		runtime.noteExecutionStart("long-running");
		for (let index = 0; index < 2_100; index++) {
			runtime.noteExecutionStart(`short-${index}`);
			runtime.noteExecutionEnd(`short-${index}`);
		}
		assert.ok(runtime.timing("long-running"));
		assert.equal(runtime.timing("long-running")!.endedAt, undefined);
		runtime.clearTimings();
		for (let index = 0; index < 2_100; index++) runtime.noteExecutionStart(`active-${index}`);
		const timings = (globalThis as Record<symbol, Map<string, unknown>>)[Symbol.for("pi.compact-tools.execution-timings")]!;
		assert.equal(timings.size, 2_000);
		assert.ok(runtime.timing("active-2099"));
	} finally { runtime.reset(true); }
});

for (const style of ["compact", "claude", "codex"] as const) test(`existing real host tool rows reuse unchanged results and draw the same under any theme (${style})`, () => {
	const previous = process.env.PI_CODING_AGENT_DIR;
	const directory = mkdtempSync(join(tmpdir(), "compact-tools-theme-reuse-"));
	process.env.PI_CODING_AGENT_DIR = directory;
	writeFileSync(join(directory, "compact-tools.json"), JSON.stringify({ style, tools: ["read", "write", "edit"], auto_compact: { read: false, write: false } }));
	const definitions = new Map<string, ToolDefinition<any, any, any>>();
	const handlers = new Map<string, Function>();
	try {
		compactTools({
			on: (name: string, handler: Function) => handlers.set(name, handler),
			registerTool: (definition: ToolDefinition<any, any, any>) => definitions.set(definition.name, definition),
			registerMarkdownTransformer() {}, registerCommand() {}, registerShortcut() {},
		} as unknown as ExtensionAPI);
		for (const definition of uiRenderers()) definitions.set(definition.name, definition as unknown as ToolDefinition<any, any, any>);
		const cases = [
			{ name: "read", args: { path: "a.ts" }, result: { content: [{ type: "text" as const, text: "const a = 1;" }], details: undefined } },
			{ name: "write", args: { path: "a.ts", content: "const a = 1;" }, result: { content: [{ type: "text" as const, text: "Written" }], details: undefined } },
			{ name: "edit", args: { path: "a.ts" }, result: { content: [{ type: "text" as const, text: "Edited" }], details: { diff: "-1 const a = 1;\n+1 const a = 2;" } } },
		];
		for (const { name, args, result } of cases) {
			initTheme("dark", false);
			const row = new ToolExecutionComponent(name, `theme-${name}`, args, { showImages: false }, definitions.get(name), { requestRender() {} } as never, directory);
			row.setArgsComplete();
			row.setExpanded(true);
			row.updateResult({ ...result, isError: false });
			const internals = row as unknown as { resultRendererComponent: unknown };
			const first = internals.resultRendererComponent;
			const dark = row.render(80);
			row.invalidate();
			assert.equal(internals.resultRendererComponent, first, `${name}: same-theme result is reused`);
			initTheme("light", false);
			row.invalidate();
			assert.equal(internals.resultRendererComponent, first, `${name}: indices do not depend on the theme, so the result is reused`);
			const light = row.render(80);
			assert.deepEqual(light.map(stripTerminalSequences), dark.map(stripTerminalSequences), `${name}: text and layout must not change`);
		}
	} finally {
		handlers.get("session_shutdown")?.({ reason: "shutdown" });
		if (previous === undefined) delete process.env.PI_CODING_AGENT_DIR;
		else process.env.PI_CODING_AGENT_DIR = previous;
		rmSync(directory, { recursive: true, force: true });
	}
});

test("missing and malformed configuration keep defaults without reading untrusted project files", () => {
	const previous = process.env.PI_CODING_AGENT_DIR;
	const directory = mkdtempSync(join(tmpdir(), "compact-tools-config-errors-"));
	process.env.PI_CODING_AGENT_DIR = directory;
	const errors: unknown[][] = [];
	const error = console.error;
	console.error = (...args) => errors.push(args);
	try {
		assert.equal(loadConfig().previewLines, 10);
		assert.equal(errors.length, 0, "a missing config is not an error");
		mkdirSync(join(directory, ".pi"));
		writeFileSync(join(directory, ".pi", "compact-tools.json"), "{");
		loadConfig(directory, false);
		assert.equal(errors.length, 0, "untrusted project file is not read");
		assert.equal(loadConfig(directory, true).previewLines, 10);
		assert.equal(errors.length, 1, "malformed trusted config warns once");
		writeFileSync(join(directory, "compact-tools.json"), "null");
		assert.equal(loadConfig().previewLines, 10);
		assert.equal(errors.length, 2, "non-object config warns");
	} finally {
		console.error = error;
		if (previous === undefined) delete process.env.PI_CODING_AGENT_DIR;
		else process.env.PI_CODING_AGENT_DIR = previous;
		rmSync(directory, { recursive: true, force: true });
	}
});

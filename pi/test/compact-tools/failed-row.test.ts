/**
 * The failed row, as docs/contract.md section 6.9 "The failed row" fixes it (hs-d590): a collapsed failed
 * row's closing row carries a reason of fewer than width/2 cells; the full error stays inside the fold.
 */
import "./pinned-themes.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { announce, loadExtension, makeRow, restoreClocks, setNow, shutdown, text } from "./indicator-harness.ts";
import { Container, visibleWidth, type Component } from "@earendil-works/pi-tui";
import type { ExtensionContext } from "@earendil-works/pi-coding-agent";
import { DEFAULT_CONFIG } from "../../compact-tools/extensions/compact-tools-config.ts";
import { ToolGroupController } from "../../compact-tools/extensions/compact-tools-grouping.ts";
import { ToolRuntime } from "../../compact-tools/extensions/compact-tools-runtime.ts";
import { theme as piTheme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

// The machine-local time is the process timezone's: pin it, so `03:11` is the same on every machine.
process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");

const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");
const REFUSAL = "φ: ask_user_question is for a few short choices; write anything larger to a file outside the repo and point to it in chat; refused: 1789 characters (limit 1500)";
const LONG = "TypeError: cannot read properties of undefined while compiling the project, see the diagnostic output above";
const SEPARATOR = " \u00B7 ";
let sequence = 0;

async function failedRow(name: string, args: object, error: string, options: { expand?: boolean } = {}) {
	const harness = await loadExtension({ style: "compact", fold: "toggle" }, { defaults: true });
	if (name === "ask_user_question") {
		harness.definitions.set(name, { name, label: name, description: "ask", parameters: { type: "object", properties: {} }, execute: async () => text("") });
	}
	const id = `failed-${++sequence}`;
	announce(harness, id, name, args);
	const row = makeRow(name, id, args, harness.definitions.get(name));
	row.setArgsComplete();
	setNow(TIME_MS);
	harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: name, args });
	row.markExecutionStarted();
	harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: true });
	row.updateResult({ ...text(error), isError: true } as never, false);
	if (options.expand) row.setExpanded(true);
	return { row, harness };
}

/** The closing row of a rendered row, SGR stripped: from its "Failed in" line on, a wrapped tail joined on. */
const closing = (lines: string[]) => {
	const shown = lines.map(plain).filter((line) => line.trim() !== "");
	const start = shown.findLastIndex((line) => /Failed in/u.test(line));
	assert.ok(start >= 0, `a closing row: ${JSON.stringify(shown)}`);
	return [shown[start]!, ...shown.slice(start + 1).map((line) => line.trim())].join(" ");
};

/** The reason after the time on a closing row, or undefined when there is none. */
function reasonOf(line: string): string | undefined {
	const match = /Failed in [0-9.]+m?s(?: \u00B7 (.*))?$/u.exec(line);
	assert.ok(match, `a closing row with a time: ${JSON.stringify(line)}`);
	return match[1];
}

function assertCapped(reason: string | undefined, width: number, label: string) {
	assert.ok(reason !== undefined, `${label}: a reason is shown at width ${width}`);
	assert.ok(2 * visibleWidth(reason) < width, `${label}: ${visibleWidth(reason)} cells is not below half of ${width}: ${JSON.stringify(reason)}`);
	assert.ok(reason.endsWith("\u2026"), `${label}: a cut reason ends in an ellipsis: ${JSON.stringify(reason)}`);
}

test("collapsed failed row cuts the reason below half the width", async () => {
	for (const width of [60, 80, 81, 100, 120]) {
		const { row, harness } = await failedRow("bash", { command: "npm test" }, LONG);
		assertCapped(reasonOf(closing(row.render(width))), width, "bash");
		shutdown(harness);
	}
	const expected: Record<number, string> = {
		60: "φ: ask_user_question is for \u2026",
		80: "φ: ask_user_question is for a few shor\u2026",
		100: "φ: ask_user_question is for a few short choices;\u2026",
	};
	for (const [width, reason] of Object.entries(expected)) {
		const { row, harness } = await failedRow("ask_user_question", {}, REFUSAL);
		assert.equal(reasonOf(closing(row.render(Number(width)))), reason, `refusal at ${width}`);
		shutdown(harness);
	}
	{
		const { row, harness } = await failedRow("ask_user_question", {}, REFUSAL);
		const line = closing(row.render(80));
		assert.ok(line.startsWith("\u2570"), line);
		assert.match(line, /^\u2570\S+ expand \u00B7 03:11 \u00B7 Failed in [0-9.]+m?s \u00B7 φ: ask_user_question is for a few shor\u2026$/u);
		shutdown(harness);
	}
	{
		const short = "twenty cells of text"; // 20 cells
		assert.equal(visibleWidth(short), 20);
		const { row, harness } = await failedRow("bash", { command: "npm test" }, LONG);
		const cut = reasonOf(closing(row.render(80)));
		assert.ok(cut !== undefined && 2 * visibleWidth(cut) < 80, "the long reason is cut first");
		shutdown(harness);
		const whole = await failedRow("bash", { command: "npm test" }, short);
		assert.equal(reasonOf(closing(whole.row.render(80))), short);
		shutdown(whole.harness);
	}
	{
		const { row, harness } = await failedRow("bash", { command: "npm test" }, LONG);
		const flat = row.render(2).map(plain).join("").replace(/\s+/gu, "");
		for (const word of ["TypeError", "diagnostic"]) assert.ok(!flat.includes(word), `no reason at width 2: ${flat}`);
		assert.ok(!/Failedin[0-9.]+m?s\u00B7/u.test(flat), `no reason separator after the time at width 2: ${flat}`);
		shutdown(harness);
	}
});

test("every tool kind gets the same cap: file, shell and custom", async () => {
	const kinds: Array<[string, object]> = [
		["read", { path: "src/app.ts" }],
		["bash", { command: "npm test" }],
		["ask_user_question", {}],
	];
	for (const [name, args] of kinds) {
		for (const width of [80, 100]) {
			const { row, harness } = await failedRow(name, args, REFUSAL);
			assertCapped(reasonOf(closing(row.render(width))), width, name);
			shutdown(harness);
		}
	}
});

test("collapsed group failure line is cut below half the width", async () => {
	const harness = await loadExtension({ style: "claude" }, { tui: true, idle: true });
	const runtime = new ToolRuntime();
	runtime.configure({ ...DEFAULT_CONFIG, style: "claude" });
	const controller = new ToolGroupController(runtime);
	const chat = new Container();
	let factory: ((tui: unknown) => Component) | undefined;
	controller.bind({ ui: { theme: piTheme, setWidget: (_key: string, value: unknown) => { factory = value as typeof factory; } } } as unknown as ExtensionContext);
	factory!({ children: [{ children: [chat] }], requestRender() {} });
	try {
		const row = makeRow("read", `g-${++sequence}`, { path: "src/0.ts" }, harness.definitions.get("read"));
		row.updateResult({ ...text(REFUSAL), isError: true } as never);
		chat.children = [row] as unknown as Component[];
		const lines = chat.render(80).map(plain);
		const failure = lines.find((line) => line.includes("Error: "));
		assert.ok(failure, `a failure line: ${JSON.stringify(lines)}`);
		const after = failure.slice(failure.indexOf("Error: ") + "Error: ".length).trimEnd();
		assert.ok(2 * visibleWidth(after) < 80, `${visibleWidth(after)} cells is not below half of 80: ${JSON.stringify(after)}`);
		assert.ok(after.endsWith("\u2026"), after);
	} finally {
		controller.dispose();
		shutdown(harness);
	}
});

test("the full error is inside the fold: collapsed cuts it, expanded shows it whole", async () => {
	const collapsed = await failedRow("ask_user_question", {}, REFUSAL);
	const lines = collapsed.row.render(100).map(plain);
	assert.ok(!lines.join("\n").includes(REFUSAL), "the collapsed row does not hold the whole refusal");
	assert.ok(2 * visibleWidth(reasonOf(closing(lines)) ?? "") < 100, "the collapsed reason is below half the width");
	shutdown(collapsed.harness);

	const { row, harness } = await failedRow("ask_user_question", {}, REFUSAL);
	row.setExpanded(true);
	const body = row.render(100).map(plain).filter((line) => line.trim() !== "");
	const last = body.at(-1)!;
	assert.match(last, /^\u2570\S+ collapse \u00B7 03:11 \u00B7 Failed in [0-9.]+m?s$/u, last);
	const joined = body.slice(0, -1).map((line) => line.replace(/^\s*\u2502 ?/u, "").trim()).join(" ");
	assert.ok(joined.replace(/\s+/gu, " ").includes(REFUSAL), `the body holds the whole error: ${JSON.stringify(joined)}`);
	shutdown(harness);
});

test.after(restoreClocks);

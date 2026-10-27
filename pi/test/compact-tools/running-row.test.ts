/**
 * The closing row of a running tool row (contract 6.1.1): with fold "toggle" it is "╰" in 8 and one loader frame
 * in 6 (the moon phases, loader-frames.ts), no text (running-loader.test.ts). Fold "status" keeps " └ Running".
 */
import assert from "node:assert/strict";
import test from "node:test";
import { MOON_FRAMES } from "./loader-frames.ts";
import { advance, announce, CASES, loadExtension, makeRow, restoreClocks, shutdown } from "./indicator-harness.ts";

const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");
const GLYPH = `[${MOON_FRAMES.join("")}]`;
const HEAD = "\u256D\u2500";
let sequence = 0;

async function running(fold: "toggle" | "status", elapsedMs = 0): Promise<string[]> {
	const harness = await loadExtension({ style: "compact", fold }, { defaults: true });
	try {
		const id = `rr-${++sequence}`;
		const args = CASES.bash!.args;
		announce(harness, id, "bash");
		const row = makeRow("bash", id, args, harness.definitions.get("bash"));
		row.setArgsComplete();
		harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: "bash", args });
		row.markExecutionStarted();
		row.updateResult({ ...(CASES.bash!.partial ?? { content: [] }), isError: false } as never, true);
		if (elapsedMs > 0) advance(elapsedMs);
		return row.render(100).filter((line) => plain(line).trim() !== "");
	} finally {
		shutdown(harness);
	}
}

test("a running tool row closes with the line in 8 and one loader frame in 6", async () => {
	const lines = await running("toggle");
	const close = lines[lines.length - 1]!;
	assert.match(plain(close), new RegExp(`^\u2570${GLYPH}$`, "u"), JSON.stringify(close));
	assert.ok(close.includes("\x1b[90m") && close.includes("\x1b[36m"), `8 and 6: ${JSON.stringify(close)}`);
	assert.ok(!close.includes("\u2514"), "no └");
});

test("the running closing row keeps its column with the head and the rails", async () => {
	const lines = await running("toggle");
	const rows = lines.map(plain);
	assert.ok(rows[0]!.startsWith(HEAD), JSON.stringify(rows[0]));
	const close = rows[rows.length - 1]!;
	assert.ok(new RegExp(`^\u2570${GLYPH}`, "u").test(close), JSON.stringify(close));
	assert.equal(rows[0]!.indexOf("\u256D"), close.indexOf("\u2570"));
	for (const body of rows.slice(1, -1)) assert.ok(body.startsWith("\u2502 "), JSON.stringify(body));
});

test("no elapsed time or Running text shows in the closing row", async () => {
	const lines = await running("toggle", 5000);
	const close = lines[lines.length - 1]!;
	assert.match(plain(close), new RegExp(`^\u2570${GLYPH}$`, "u"), JSON.stringify(plain(close)));
	assert.ok(close.includes("\x1b[90m") && close.includes("\x1b[36m") && !close.includes("\x1b[37m"), JSON.stringify(close));
});

test("a finished row closes with the toggle in 7 and the line in 8", async () => {
	const harness = await loadExtension({ style: "compact", fold: "toggle" }, { defaults: true });
	try {
		const id = `rr-${++sequence}`;
		const outcome = CASES.bash!;
		announce(harness, id, "bash");
		const row = makeRow("bash", id, outcome.args, harness.definitions.get("bash"));
		row.setArgsComplete();
		harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: "bash", args: outcome.args });
		row.markExecutionStarted();
		advance(300);
		harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: false });
		row.updateResult({ ...outcome.success, isError: false } as never, false);
		const lines = row.render(100).filter((line) => plain(line).trim() !== "");
		const close = lines[lines.length - 1]!;
		assert.ok(close.includes("\x1b[90m\u2570"), `line in 8: ${JSON.stringify(close)}`);
		assert.match(close, /\x1b\[37m[^\x1b]*[\u{F0B26}\u{F0B2C}]/u, `toggle in 7: ${JSON.stringify(close)}`);
		assert.ok(!new RegExp(GLYPH, "u").test(close), "no loader frame once finished");
		// The running row of the same tool is not a toggle: its closing row has no 7.
		const live = await running("toggle");
		assert.ok(new RegExp(`^\u2570${GLYPH}$`, "u").test(plain(live[live.length - 1]!)), "running row closes with a loader frame");
		assert.ok(!live[live.length - 1]!.includes("\x1b[37m"), "running row has no toggle colour");
	} finally {
		shutdown(harness);
	}
});

test("fold status keeps the running └ row", async () => {
	const lines = await running("status");
	const close = plain(lines[lines.length - 1]!);
	assert.match(close, /^ \u2514 Running$/u, JSON.stringify(close));
	// Merged guard: with fold toggle the same row must no longer be └ (red today).
	const toggled = plain((await running("toggle")).at(-1)!);
	assert.ok(!toggled.includes("\u2514") && new RegExp(GLYPH, "u").test(toggled), JSON.stringify(toggled));
});

test.after(restoreClocks);

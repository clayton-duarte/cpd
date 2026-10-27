/**
 * The closing row of a running tool row (docs/contract.md 6.1.1, 6.9): "╰" in 8 and ONE frame of pi's moon-phase loader
 * in 6 (loader-frames.ts, one frame every 50 ms), no text after it. A finished row closes with its
 * toggle in 7; fold "status" keeps " └ Running".
 */
import assert from "node:assert/strict";
import test from "node:test";
import { MOON_FRAMES } from "./loader-frames.ts";
import { advance, announce, CASES, loadExtension, makeRow, restoreClocks, shutdown } from "./indicator-harness.ts";

const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");
const FRAMES = MOON_FRAMES;
const OLD_GLYPH = "\u{F0B2A}";
let sequence = 0;

async function withRow<T>(
	fold: "toggle" | "status",
	command: string | undefined,
	body: (row: ReturnType<typeof makeRow>) => T,
	finish = false,
): Promise<T> {
	const harness = await loadExtension({ style: "compact", fold }, { defaults: true });
	try {
		const id = `rl-${++sequence}`;
		const outcome = CASES.bash!;
		const args = command === undefined ? outcome.args : { command };
		announce(harness, id, "bash");
		const row = makeRow("bash", id, args, harness.definitions.get("bash"));
		row.setArgsComplete();
		harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: "bash", args });
		row.markExecutionStarted();
		if (finish) {
			advance(300);
			harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: false });
			row.updateResult({ ...outcome.success, isError: false } as never, false);
		} else {
			row.updateResult({ ...(outcome.partial ?? { content: [] }), isError: false } as never, true);
		}
		return body(row);
	} finally {
		shutdown(harness);
	}
}

const drawn = (row: ReturnType<typeof makeRow>) => row.render(100).filter((line) => plain(line).trim() !== "");
const closing = (row: ReturnType<typeof makeRow>) => drawn(row).at(-1)!;

function assertLoaderRow(close: string): void {
	const text = plain(close);
	assert.match(text, new RegExp(`^\u2570[${MOON_FRAMES.join("")}]$`, "u"), JSON.stringify(close));
	assert.ok(close.includes("\x1b[90m\u2570"), `line in 8: ${JSON.stringify(close)}`);
	const frame = [...text].at(-1)!;
	const at = close.indexOf(frame);
	assert.ok(/\x1b\[36m$/u.test(close.slice(0, at)), `frame in 6: ${JSON.stringify(close)}`);
	assert.ok(!close.includes("\x1b[37m"), `no toggle colour: ${JSON.stringify(close)}`);
}

test("a running tool row closes with a line in 8 and a loader frame in 6", async () => {
	assertLoaderRow(await withRow("toggle", undefined, closing));
});

test("a running group head has the same closing row and never an open rail", async () => {
	await withRow("toggle", "date; ls; pwd; whoami", (row) => {
		const lines = drawn(row);
		assert.match(plain(lines[0]!), /^\u256D\u2500.*Run date \+ \d+ more steps/u, JSON.stringify(plain(lines[0]!)));
		assert.ok(lines.length >= 2, `an open rail: ${JSON.stringify(lines.map(plain))}`);
		assertLoaderRow(lines.at(-1)!);
	});
});

test("the loader steps through the 28 moon_alt phases one every 50 ms", async () => {
	await withRow("toggle", undefined, (row) => {
		const seen: string[] = [];
		for (let step = 0; step < 58; step++) {
			seen.push([...plain(closing(row))].at(-1)!);
			advance(50);
		}
		assert.ok(seen.every((frame) => FRAMES.includes(frame)), JSON.stringify(seen));
		const start = FRAMES.indexOf(seen[0]!);
		const expected = seen.map((_, i) => FRAMES[(start + i) % 28]);
		assert.deepEqual(seen, expected, "each 50 ms is the next frame, in order, wrapping after 28");
		advance(25);
		const before = [...plain(closing(row))].at(-1);
		advance(20);
		assert.equal([...plain(closing(row))].at(-1), before, "no step inside 50 ms");
	});
});

test("the running closing row has no text after the glyph", async () => {
	await withRow("toggle", undefined, (row) => {
		for (let step = 0; step < 3; step++) {
			const close = plain(closing(row));
			assert.ok(!close.includes("Running") && !/\d+(\.\d+)?m?s/u.test(close), JSON.stringify(close));
			assert.equal([...close].length, 2, `"╰" and one frame only: ${JSON.stringify(close)}`);
			assert.ok(!close.includes(OLD_GLYPH), "no U+F0B2A");
			advance(5000);
		}
	});
});

test("a finished row closes with its toggle as before", async () => {
	await withRow("toggle", undefined, (row) => {
		const close = closing(row);
		assert.ok(close.includes("\x1b[90m\u2570"), `line in 8: ${JSON.stringify(close)}`);
		assert.match(close, /\x1b\[37m[^\x1b]*[\u{F0B26}\u{F0B2C}]/u, `toggle in 7: ${JSON.stringify(close)}`);
		assert.ok(!FRAMES.some((frame) => close.includes(frame)), "no loader frame once finished");
	}, true);
	// Merged guard (red today): the running row of the same tool is the loader row, not the toggle.
	assertLoaderRow(await withRow("toggle", undefined, closing));
});

test("fold status keeps the running └ row", async () => {
	await withRow("status", undefined, (row) => {
		assert.match(plain(closing(row)), /^ \u2514 Running$/u, JSON.stringify(plain(closing(row))));
	});
	// Merged guard (red today): with fold toggle the same row is the loader row, not └ Running.
	const toggled = plain(await withRow("toggle", undefined, closing));
	assert.ok(!toggled.includes("Running") && !toggled.includes("\u2514"), JSON.stringify(toggled));
});

test.after(restoreClocks);

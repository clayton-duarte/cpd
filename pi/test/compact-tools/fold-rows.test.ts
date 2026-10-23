/**
 * Tool and command rows (docs/contract.md sections 6.1.1 and 6.9, hs-d557 items 3 and 5): every row starts
 * with one space; the head is ` ╭─` and the pill with no dot; body rows ` │ `, closing row ` ╰`; lead and
 * rails are 8, the toggle is 7, pills use only 0-7. Indexed SGR only.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { advance, announce, CASES, loadExtension, makeRow, restoreClocks, shutdown, text } from "./indicator-harness.ts";

const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");
const LEFT = "\uE0B6";
const RIGHT = "\uE0B4";
const HEAD = " \u256D\u2500"; // " ╭─"

let sequence = 0;

type Phase = "running" | "done";

async function rowLines(phase: Phase, expanded = false, command?: string): Promise<string[]> {
	const harness = await loadExtension({ style: "compact", fold: "toggle" }, { defaults: true });
	try {
		const id = `fr-${++sequence}`;
		const outcome = CASES.bash!;
		const args = command === undefined ? outcome.args : { command };
		announce(harness, id, "bash");
		const row = makeRow("bash", id, args, harness.definitions.get("bash"));
		row.setArgsComplete();
		harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: "bash", args });
		row.markExecutionStarted();
		if (phase === "done") {
			advance(300);
			harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: false });
			row.updateResult({ ...outcome.success, isError: false } as never, false);
		}
		if (expanded) row.setExpanded(true);
		return row.render(100).filter((line) => plain(line).trim() !== "");
	} finally {
		shutdown(harness);
	}
}

const hasDot = (lines: string[]) => lines.some((line) => /\u2981|\u25CF|\u29BF|\u2022/u.test(plain(line)));

test("a tool row's title row is a space, the lead and the pill with no dot", async () => {
	const lines = await rowLines("done");
	const head = plain(lines[0]!);
	assert.ok(head.startsWith(`${HEAD}${LEFT}`), `head ${JSON.stringify(head)}`);
	assert.ok(!hasDot(lines), JSON.stringify(lines.map(plain)));
	assert.ok(!/\u2981/u.test(lines.join("")), "no dot glyph U+2981");
});

test("a running tool row has no dot, only the cyan pill", async () => {
	const lines = await rowLines("running");
	const head = lines[0]!;
	assert.ok(plain(head).startsWith(`${HEAD}${LEFT}`), `head ${JSON.stringify(plain(head))}`);
	assert.ok(!/\u2981/u.test(lines.join("")), "no dot glyph U+2981");
	assert.ok(head.includes("\x1b[46m"), `pill on bg 6: ${JSON.stringify(head)}`);
	assert.ok(head.includes("\x1b[36m"), `cap in fg 6: ${JSON.stringify(head)}`);
});

test("head, body and closing rows of an expanded tool row share one column", async () => {
	const lines = await rowLines("done", true);
	assert.ok(lines.length >= 3, JSON.stringify(lines.map(plain)));
	const rows = lines.map(plain);
	assert.ok(rows.every((row) => row.startsWith(" ")), `one leading space: ${JSON.stringify(rows)}`);
	assert.ok(rows[0]!.startsWith(`${HEAD}${LEFT}`), JSON.stringify(rows[0]));
	const close = rows[rows.length - 1]!;
	assert.ok(close.startsWith(" \u2570"), `closing row ${JSON.stringify(close)}`);
	for (const body of rows.slice(1, -1)) assert.ok(body.startsWith(" \u2502 "), `body row ${JSON.stringify(body)}`);
	// The glyph of every row sits in column 1.
	assert.equal(rows[0]!.indexOf("\u256D"), close.indexOf("\u2570"));
});

test("a grouped command row has the same head and no dot", async () => {
	const lines = await rowLines("done", false, "date; ls; pwd; whoami");
	const head = plain(lines[0]!);
	assert.ok(head.startsWith(`${HEAD}${LEFT}`), `head ${JSON.stringify(head)}`);
	assert.match(head, /bash.*Run date \+ \d+ more steps/u);
	assert.ok(!/\u2981/u.test(lines.join("")), "no dot glyph U+2981");
	const close = plain(lines[lines.length - 1]!);
	assert.ok(close.startsWith(" \u2570"), `closing row ${JSON.stringify(close)}`);
	assert.equal(head.indexOf("\u256D"), close.indexOf("\u2570"), "head and closing row share a column");
});

test("lead and rails are 8 and the toggle is 7 on every tool row", async () => {
	for (const expanded of [false, true]) {
		const lines = await rowLines("done", expanded);
		const head = lines[0]!;
		assert.ok(/\x1b\[90m ?\u256D\u2500/u.test(head), `lead " ╭─" in 8: ${JSON.stringify(head)}`);
		for (const line of lines.slice(1)) {
			const glyph = plain(line).trimStart()[0];
			if (glyph === "\u2502" || glyph === "\u2570") {
				assert.ok(line.includes(`\x1b[90m${glyph}`) || line.includes(`\x1b[90m ${glyph}`), `${glyph} in 8: ${JSON.stringify(line)}`);
			}
		}
		const close = lines.find((line) => /[\u{F0B26}\u{F0B2C}]/u.test(line))!;
		assert.match(close, /\x1b\[37m[^\x1b]*(\u{F0B26}|\u{F0B2C})/u, `toggle in 7: ${JSON.stringify(close)}`);
	}
});

test("no pill or toggle uses a colour above 7", async () => {
	for (const phase of ["running", "done"] as const) {
		for (const expanded of [false, true]) {
			const lines = await rowLines(phase, expanded);
			const head = lines[0]!;
			assert.ok(plain(head).startsWith(`${HEAD}${LEFT}`), `head ${JSON.stringify(plain(head))}`);
			const start = head.indexOf(LEFT);
			assert.ok(start >= 0, `a pill in ${JSON.stringify(head)}`);
			const end = head.indexOf(RIGHT, start);
			const pill = head.slice(Math.max(0, start - 12), end + 1);
			for (const code of pill.matchAll(/\x1b\[([0-9;]*)m/gu)) {
				for (const n of code[1]!.split(";").map(Number)) {
					assert.ok(!(n >= 90 && n <= 107), `bright SGR ${n} in the pill ${JSON.stringify(pill)}`);
				}
			}
			if (phase === "running") continue;
			const close = lines.find((line) => /[\u{F0B26}\u{F0B2C}]/u.test(line));
			assert.ok(close, `a toggle in ${JSON.stringify(lines)}`);
			assert.ok(!/\x1b\[(9\d|10\d)m[^\x1b]*[\u{F0B26}\u{F0B2C}]/u.test(close), `toggle above 7: ${JSON.stringify(close)}`);
		}
	}
});

test.after(restoreClocks);

/**
 * The ask pill (docs/contract.md 6.1.1, 6.9): `[ask_user_question]` is magenta (bg 5, text 0, caps 5) exactly while the
 * tool executes and has no result (the window of the closing row ` ╰󰬦 choose`); cyan 6 before; 2 / 1 after; no other tool's
 * pill is ever magenta. The real ToolExecutionComponent is driven through indicator-harness.ts; the head row's raw SGR is read.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { advance, announce, CASES, loadExtension, makeRow, shutdown, text } from "./indicator-harness.ts";

const ESC = "\x1b[";
const pillOf = (n: number, name: string) =>
	`${ESC}${30 + n}m\uE0B6${ESC}7m${name}${ESC}27m\uE0B4${ESC}39m`;
const ARGS = { questions: [{ question: "Which one?", header: "Pick", options: ["a", "b"] }] };
let sequence = 0;

type Stage = "streaming" | "started" | "partial" | "success" | "failure";

async function head(name: string, stage: Stage, args: Record<string, unknown> = ARGS): Promise<string> {
	const harness = await loadExtension({ style: "compact", fold: "toggle" }, { defaults: true });
	try {
		const id = `askpill-${++sequence}`;
		announce(harness, id, name);
		const row = makeRow(name, id, {}, name === "ask_user_question" ? undefined : harness.definitions.get(name));
		row.updateArgs(args);
		if (stage !== "streaming") {
			row.setArgsComplete();
			advance(250);
			harness.handlers.get("tool_execution_start")?.({ toolCallId: id, toolName: name, args });
			row.markExecutionStarted();
		}
		if (stage === "partial") row.updateResult({ ...text("partial"), isError: false } as never, true);
		if (stage === "success" || stage === "failure") {
			const failed = stage === "failure";
			advance(1_000);
			harness.handlers.get("tool_execution_end")?.({ toolCallId: id, toolName: name, isError: failed });
			row.updateResult({ ...text(failed ? "Question cancelled" : "User answered: a"), isError: failed } as never, false);
		}
		const lines = row.render(100).filter((line) => line.replace(/\x1b\[[0-9;]*m/gu, "").trim() !== "");
		console.log(name, stage, JSON.stringify(lines[0]));
		return lines[0]!;
	} finally {
		shutdown(harness);
	}
}

test("the ask_user_question pill is magenta from the start of its execution until its result", async () => {
	for (const stage of ["started", "partial"] as const) {
		const line = await head("ask_user_question", stage);
		assert.ok(line.includes(pillOf(5, "ask_user_question")), `${stage}: magenta pill in ${JSON.stringify(line)}`);
	}
});

test("before its execution the ask_user_question pill keeps the running colour", async () => {
	const line = await head("ask_user_question", "streaming");
	assert.ok(line.includes(pillOf(6, "ask_user_question")), `cyan pill in ${JSON.stringify(line)}`);
	assert.ok(!line.includes(`${ESC}45m`), "not magenta while the arguments stream");
	// ...and the pill turns magenta once the execution starts.
	const started = await head("ask_user_question", "started");
	assert.ok(started.includes(pillOf(5, "ask_user_question")), `magenta once started: ${JSON.stringify(started)}`);
});

test("an answered or failed ask_user_question takes the finished colours", async () => {
	const done = await head("ask_user_question", "success");
	const failed = await head("ask_user_question", "failure");
	assert.ok(done.includes(pillOf(2, "ask_user_question")), `green in ${JSON.stringify(done)}`);
	assert.ok(failed.includes(pillOf(1, "ask_user_question")), `red in ${JSON.stringify(failed)}`);
	// ...and the magenta window is what distinguishes it: it was magenta while open.
	const open = await head("ask_user_question", "started");
	assert.ok(open.includes(`${ESC}45m`), `magenta while open in ${JSON.stringify(open)}`);
});

test("no other tool's pill turns magenta while it runs", async () => {
	// Guard against a blanket change: the ask pill must be magenta while open AND bash must not be.
	const open = await head("ask_user_question", "started");
	assert.ok(open.includes(`${ESC}45m`), `ask is magenta while open: ${JSON.stringify(open)}`);
	for (const [name, outcome] of Object.entries(CASES).filter(([key]) => key !== "powershell")) {
		for (const stage of ["streaming", "started", "partial"] as const) {
			const line = await head(name, stage, outcome.args);
			assert.ok(!line.includes(`${ESC}45m`) && !line.includes(`${ESC}35m`), `${name}/${stage} not magenta: ${JSON.stringify(line)}`);
			assert.ok(line.includes(`${ESC}46m`), `${name}/${stage} keeps the running colour: ${JSON.stringify(line)}`);
		}
	}
});

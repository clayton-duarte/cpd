/** Pi-side ask-answer diagnostics: the diag log lines of mountQuestionnaire, through the askDiagIo seam. */
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { mock } from "node:test";

const MOD = await import("../ask-user-question/ask-user-question.ts");
const SECRET = "SECRET-custom-answer-text";
const QUESTIONS = { questions: [{ header: "Choice", question: "Proceed?", options: [
	{ label: "Yes", description: "Continue" }, { label: "No", description: "Stop" },
] }] };

type Line = Record<string, unknown>;

/** Run one tool call with an external answer file `fileText` (or none); returns the tool result and what the diag io saw. */
async function run(fileText: string | undefined, io: (file: string, line: string) => void, settles = true) {
	const home = mkdtempSync(join(tmpdir(), "ask-diag-"));
	const saved = { home: process.env.CPD_HOME, session: process.env.CPD_SESSION };
	process.env.CPD_HOME = home;
	process.env.CPD_SESSION = "s1";
	const original = MOD.askDiagIo.append;
	MOD.askDiagIo.append = io;
	mock.timers.enable({ apis: ["setInterval", "setTimeout"] });
	try {
		let tool: { execute: (...a: unknown[]) => Promise<unknown> } | undefined;
		const pi = new Proxy({ events: { emit: () => undefined } } as Record<string, unknown>, {
			get: (t, method: string) => (method === "events" ? t.events : (...args: unknown[]) => {
				if (method === "registerTool") tool = args[0] as typeof tool;
			}),
		});
		MOD.registerAskUserQuestionTool(pi as never);
		const ctx = { hasUI: true, ui: { custom: () => new Promise(() => undefined) } };
		const pending = tool!.execute("c1", QUESTIONS, undefined, undefined, ctx);
		if (fileText !== undefined) {
			mkdirSync(join(home, "run", "ask"), { recursive: true });
			writeFileSync(join(home, "run", "ask", "s1.json"), fileText);
		}
		mock.timers.tick(120);
		return { result: settles ? await pending : undefined, home };
	} finally {
		mock.timers.reset();
		MOD.askDiagIo.append = original;
		for (const [k, v] of [["CPD_HOME", saved.home], ["CPD_SESSION", saved.session]] as const) {
			if (v === undefined) delete process.env[k]; else process.env[k] = v;
		}
		rmSync(home, { recursive: true, force: true });
	}
}

const answerFile = JSON.stringify({ dialogId: "call:q1", result: {
	answers: [{ questionIndex: 0, question: "Proceed?", kind: "custom", answer: SECRET }], cancelled: false,
} });

test("pi records mount.start and answer.consumed with kinds only", async () => {
	const lines: Line[] = [];
	const files: string[] = [];
	await run(answerFile, (file, line) => { files.push(file); lines.push(JSON.parse(line) as Line); });
	assert.ok(files.length > 0, "diagnostics were written");
	assert.ok(files.every((f) => f.endsWith(join("run", "ask-diag.log"))));
	const names = lines.map((l) => l.event);
	assert.ok(names.includes("mount.start"), `events: ${names.join(",")}`);
	const consumed = lines.find((l) => l.event === "answer.consumed");
	assert.ok(consumed, `events: ${names.join(",")}`);
	assert.equal(consumed.side, "pi");
	assert.equal(consumed.session, "s1");
	assert.deepEqual(consumed.kinds, ["custom"]);
	assert.equal(consumed.cancelled, false);
	assert.equal(typeof consumed.at, "string");
	const settled = lines.find((l) => l.event === "mount.settled");
	assert.ok(settled);
	assert.equal(settled.source, "external");
	assert.ok(!lines.some((l) => JSON.stringify(l).includes(SECRET)), "no answer text in the log");
});

test("pi records answer.parse_failed for a malformed answer file", async () => {
	const lines: Line[] = [];
	// A malformed file is dropped and the mount stays open, so the run does not await the tool call.
	await run("{not json", (_f, line) => lines.push(JSON.parse(line) as Line), false);
	const failed = lines.find((l) => l.event === "answer.parse_failed");
	assert.ok(failed, `events: ${lines.map((l) => l.event).join(",")}`);
	assert.equal(failed.error, "json_syntax");
	assert.ok(!JSON.stringify(failed).includes(SECRET));
});

test("answer.parse_failed never quotes sensitive text from a malformed file", async () => {
	const lines: Line[] = [];
	await run(`{"result":{"answers":[{"answer":${SECRET}}]}}`, (_f, line) => lines.push(JSON.parse(line) as Line), false);
	const failed = lines.find((l) => l.event === "answer.parse_failed");
	assert.ok(failed, "parse_failed recorded");
	assert.ok(!lines.some((l) => JSON.stringify(l).includes(SECRET)), "no answer text in the log");
});

test("a failing diagnostic write does not change the poll result", async () => {
	let attempts = 0;
	const { result } = await run(answerFile, () => { attempts += 1; throw new Error("disk full"); });
	assert.ok(attempts > 0, "a diagnostic write was attempted");
	const text = JSON.stringify(result);
	assert.ok(text.includes(SECRET), "the answer still resolves the tool call");
});

/**
 * The WebUI answer path (CONTRACT 11.4): with CPD_SESSION set, ask_user_question polls
 * `<CPD_HOME>/run/ask/<CPD_SESSION>.json` every 120 ms, resolves with its `result`, deletes
 * the file, and deletes a stale file at its own start. Temp CPD_HOME, fake timers, fake ctx:
 * no real pi, no sleeps.
 */
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { afterEach, beforeEach } from "node:test";
import { install } from "@sinonjs/fake-timers";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { registerAskUserQuestionTool } from "../../ask-user-question/ask-user-question.ts";

const POLL_MS = 120;
const SESSION = "sess-1";
const PARAMS = {
	questions: [
		{
			question: "Which one?",
			header: "Pick",
			options: [
				{ label: "a", description: "Pick the first option" },
				{ label: "b", description: "Pick the second option" },
			],
		},
	],
};
const ANSWER = {
	answers: [{ questionIndex: 0, question: "Which one?", kind: "option", answer: "b" }],
	cancelled: false,
};

type Execute = (id: string, params: unknown, signal: AbortSignal, onUpdate: () => void, ctx: ExtensionContext) => Promise<any>;

const KEYS = ["CPD_HOME", "CPD_SESSION", "CPD_PLAN", "CPD_ATTEMPT", "XDG_CONFIG_HOME"] as const;
let saved: Array<readonly [string, string | undefined]>;
let home: string;
let clock: ReturnType<typeof install>;

beforeEach(() => {
	saved = KEYS.map((k) => [k, process.env[k]] as const);
	home = mkdtempSync(join(tmpdir(), "ask-webui-"));
	for (const k of KEYS) delete process.env[k];
	process.env.CPD_HOME = join(home, "cpd");
	process.env.CPD_SESSION = SESSION;
	process.env.XDG_CONFIG_HOME = join(home, "xdg");
	clock = install({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
});
afterEach(() => {
	clock.uninstall();
	for (const [k, v] of saved) {
		if (v === undefined) delete process.env[k];
		else process.env[k] = v;
	}
	rmSync(home, { recursive: true, force: true });
});

const answerFile = () => join(process.env.CPD_HOME as string, "run", "ask", `${SESSION}.json`);
const writeAnswer = (body: unknown) => {
	mkdirSync(join(process.env.CPD_HOME as string, "run", "ask"), { recursive: true });
	writeFileSync(answerFile(), JSON.stringify(body));
};

/** Start one execute against a ctx whose custom() never settles (the TUI is not answered). */
function start(): { out: Promise<any>; settle: () => void } {
	let execute: Execute | undefined;
	const pi = {
		registerTool: (d: { execute: Execute }) => {
			execute = d.execute;
		},
		events: { emit: () => {} },
	} as unknown as ExtensionAPI;
	registerAskUserQuestionTool(pi);
	let resolveCustom: (v: unknown) => void = () => {};
	const ctx = {
		hasUI: true,
		mode: "tui",
		cwd: "",
		isProjectTrusted: () => true,
		ui: {
			custom: () => new Promise((resolve) => (resolveCustom = resolve)),
			notify: () => {},
			onTerminalInput: () => () => {},
		},
	} as unknown as ExtensionContext;
	const out = (execute as Execute)("id", PARAMS, new AbortController().signal, () => {}, ctx);
	return { out, settle: () => resolveCustom({ answers: [], cancelled: true }) };
}

test("ask-user-question resolves from the CPD_SESSION answer file", async () => {
	const run = start();
	try {
		writeAnswer({ dialogId: "call:1", result: ANSWER });
		clock.tick(POLL_MS);
		let result: any;
		void run.out.then((r) => (result = r));
		for (let i = 0; i < 50; i++) await Promise.resolve();
		assert.ok(result !== undefined, "the poll resolves the question from the answer file");
		assert.deepEqual(result.details.answers, ANSWER.answers);
		assert.equal(result.details.cancelled, false);
		assert.equal(existsSync(answerFile()), false, "the answer file is consumed");
	} finally {
		run.settle();
		await run.out;
	}
});

test("ask-user-question removes a stale answer file at start", async () => {
	writeAnswer({ dialogId: "old", result: ANSWER });
	const run = start();
	try {
		assert.equal(existsSync(answerFile()), false, "the stale file is deleted when the question starts");
		clock.tick(POLL_MS * 2);
		let settled = false;
		void run.out.then(() => (settled = true));
		await Promise.resolve();
		assert.equal(settled, false, "a stale answer never resolves the new question");
	} finally {
		run.settle();
		await run.out;
	}
});

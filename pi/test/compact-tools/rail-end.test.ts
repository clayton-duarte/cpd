/**
 * The rail end rule (docs/contract.md 6.9, the table of closing rows): every pill head `╭─` is, at every instant of
 * the row's life, followed by exactly ONE closing row, and it is one of five forms:
 *   1/2  `╰󰬦 expand` / `╰󰬬 collapse`    the fold's toggle, glyph in 7
 *   3    `╰` in 8 + one loader frame in 6    from the row's creation (argument streaming included) until a result
 *   4    `╰󰬪` in 8                            a block that cannot fold
 *   5    `╰󰬦 choose`, the whole row in 8      `ask_user_question` only, from the start of its execution until its result
 * Rows are driven through Pi's own ToolExecutionComponent in Pi's order (indicator-harness.ts).
 */
import assert from "node:assert/strict";
import test from "node:test";
import { MOON_FRAMES } from "./loader-frames.ts";
import {
	advance, announce, CASES, CUSTOM_CASES, loadExtension, makeRow, restoreClocks, shutdown, text, type Case, type Harness,
} from "./indicator-harness.ts";

const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");
const FRAMES = MOON_FRAMES;
const LOADER = new RegExp(`^\u2570[${FRAMES.join("")}]$`, "u");
const TOGGLE = /^\u2570[\u{F0B26}\u{F0B2C}] (expand|collapse)( · .*)?$/u;
const TERMINATOR = /^\u2570\u{F0B2A}$/u;
const CHOOSE = /^\u2570\u{F0B26} choose$/u;
const WIDTHS = [40, 100];

type Row = ReturnType<typeof makeRow>;
type Form = "loader" | "toggle" | "terminator" | "choose";

const ASK: Case = {
	args: { questions: [{ question: "Which one?", options: ["a", "b"] }] },
	success: text("User answered: a"),
	failure: text("Question cancelled"),
};

/** Every tool kind: the built-ins, the custom tools (one with its own result renderer) and ask_user_question. */
const KINDS: Array<{ key: string; name: string; outcome: Case; definition: (harness: Harness) => object | undefined }> = [
	// powershell has no registered renderer in the harness, so it draws no pill head at all.
	...Object.entries(CASES).filter(([name]) => name !== "powershell").map(([name, outcome]) => ({
		key: name, name, outcome, definition: (harness: Harness) => harness.definitions.get(name),
	})),
	...Object.entries(CUSTOM_CASES).map(([key, outcome]) => {
		const name = key.split("@")[0]!;
		return { key, name, outcome, definition: () => (outcome.author ? { name, ...outcome.author } : undefined) };
	}),
	{ key: "ask_user_question", name: "ask_user_question", outcome: ASK, definition: () => undefined },
];
const ordinary = KINDS.filter((kind) => kind.name !== "ask_user_question");
const ask = KINDS.find((kind) => kind.name === "ask_user_question")!;

let sequence = 0;
const drawn = (row: Row, width: number) => row.render(width).filter((line) => plain(line).trim() !== "");

/** One closing row of a block's raw rows: parsed against the five forms, colours read from the raw bytes. */
function formOf(raw: string): Form | undefined {
	const text = plain(raw);
	const sgrs = [...raw.matchAll(/\x1b\[([0-9;]*)m/gu)].map((match) => match[1]!);
	if (TERMINATOR.test(text)) return sgrs.every((sgr) => ["90", "0", "39", ""].includes(sgr)) && sgrs.includes("90") ? "terminator" : undefined;
	if (CHOOSE.test(text)) return sgrs.every((sgr) => ["90", "0", "39", ""].includes(sgr)) && sgrs.includes("90") ? "choose" : undefined;
	if (LOADER.test(text)) {
		const frame = [...text].at(-1)!;
		const before = raw.slice(0, raw.indexOf(frame));
		const line = raw.includes("\x1b[90m\u2570");
		return line && /\x1b\[36m$/u.test(before) && !raw.includes("\x1b[37m") ? "loader" : undefined;
	}
	if (TOGGLE.test(text)) return /\x1b\[37m[^\x1b]*[\u{F0B26}\u{F0B2C}]/u.test(raw) ? "toggle" : undefined;
	return undefined;
}

/** The block's rule: it starts with `╭─`, its last non-blank row is one of the five forms and is the ONLY `╰` row. */
function closingOf(lines: string[], where: string): Form {
	assert.ok(plain(lines[0] ?? "").startsWith("\u256D\u2500"), `${where}: no pill head: ${JSON.stringify(lines.map(plain))}`);
	const closers = lines.filter((line) => plain(line).startsWith("\u2570"));
	assert.equal(closers.length, 1, `${where}: exactly one closing row, got ${closers.length}: ${JSON.stringify(lines.map(plain))}`);
	// A toggle's metadata may wrap at a narrow width: its continuation rows start in column 0 (no row of a tool block starts
	// with a margin column, test/chat-margins.test.ts) and start no rail or head.
	const at = lines.findIndex((line) => plain(line).startsWith("\u2570"));
	const rest = lines.slice(at + 1);
	assert.ok(rest.every((line) => /^[^\u256D\u2570\u2502 ]/u.test(plain(line))), `${where}: rows after the closing row: ${JSON.stringify(lines.map(plain))}`);
	const closing = lines[at]!;
	const form = formOf(closing);
	assert.ok(form, `${where}: the closing row is not one of the five forms (an open rail): ${JSON.stringify(closing)} in ${JSON.stringify(lines.map(plain))}`);
	assert.ok(rest.length === 0 || form === "toggle", `${where}: only a toggle wraps: ${JSON.stringify(lines.map(plain))}`);
	if (rest.length > 0) assert.ok(at === lines.length - 1 - rest.length);
	return form;
}

type Stage = "announced" | "streaming" | "complete" | "started" | "empty partial" | "partial" | "success" | "failure" | "aborted";
const STAGES: Stage[] = ["announced", "streaming", "complete", "started", "empty partial", "partial", "success", "failure", "aborted"];

/** A row of one kind taken to a stage of its life, in Pi's order. */
function rowAt(harness: Harness, kind: (typeof KINDS)[number], stage: Stage, expanded: boolean): Row {
	const id = `rail-${++sequence}`;
	const { name, outcome } = kind;
	announce(harness, id, name);
	const row = makeRow(name, id, {}, kind.definition(harness));
	row.setExpanded(expanded);
	if (stage === "announced") return row;
	row.updateArgs(outcome.args);
	if (stage === "streaming") return row;
	row.setArgsComplete();
	if (stage === "complete") return row;
	if (stage === "aborted") {
		// Pi aborts before the tool ran: an error result, extensions never hear the execution end.
		row.updateResult({ ...outcome.failure, isError: true } as never, false);
		return row;
	}
	advance(250);
	harness.handlers.get("tool_execution_start")?.({ toolCallId: id, toolName: name, args: outcome.args });
	row.markExecutionStarted();
	if (stage === "started") return row;
	row.updateResult({ content: [], isError: false } as never, true);
	if (stage === "empty partial") return row;
	row.updateResult({ ...(outcome.partial ?? text("partial output")), isError: false } as never, true);
	if (stage === "partial") return row;
	advance(1_234);
	const failed = stage === "failure";
	harness.handlers.get("tool_execution_end")?.({ toolCallId: id, toolName: name, isError: failed });
	row.updateResult({ ...(failed ? outcome.failure : outcome.success), isError: failed } as never, false);
	return row;
}

async function withHarness<T>(body: (harness: Harness) => T | Promise<T>): Promise<T> {
	const harness = await loadExtension({ style: "compact", fold: "toggle" }, { defaults: true });
	try {
		return await body(harness);
	} finally {
		shutdown(harness);
	}
}

const EXPECTED: Record<Stage, Form[]> = {
	announced: ["loader"], streaming: ["loader"], complete: ["loader"], started: ["loader"], "empty partial": ["loader"],
	partial: ["loader"], success: ["toggle"], failure: ["toggle"], aborted: ["toggle"],
};

test("no pill head is ever without exactly one of the five closing rows, for every tool, stage, fold state and width", async () => {
	await withHarness((harness) => {
		for (const kind of KINDS) {
			for (const stage of STAGES) {
				for (const expanded of [false, true]) {
					for (const width of WIDTHS) {
						const where = `${kind.key} / ${stage} / ${expanded ? "expanded" : "folded"} / ${width}`;
						const form = closingOf(drawn(rowAt(harness, kind, stage, expanded), width), where);
						const allowed: Form[] = kind.name === "ask_user_question" && ["started", "empty partial", "partial"].includes(stage)
							? ["choose"] : EXPECTED[stage];
						assert.ok(allowed.includes(form), `${where}: closes with ${form}, expected ${allowed.join(" or ")}`);
					}
				}
			}
		}
	});
});

test("a row whose arguments are still streaming closes with the loader", async () => {
	await withHarness((harness) => {
		for (const kind of KINDS) {
			for (const stage of ["announced", "streaming", "complete"] as const) {
				for (const width of WIDTHS) {
					const where = `${kind.key} / ${stage} / ${width}`;
					assert.equal(closingOf(drawn(rowAt(harness, kind, stage, false), width), where), "loader", where);
				}
			}
		}
	});
});

test("a started row with no result yet closes with the loader, for every tool kind", async () => {
	await withHarness((harness) => {
		for (const kind of ordinary) {
			for (const stage of ["started", "empty partial", "partial"] as const) {
				for (const width of WIDTHS) {
					const where = `${kind.key} / ${stage} / ${width}`;
					assert.equal(closingOf(drawn(rowAt(harness, kind, stage, false), width), where), "loader", where);
				}
			}
		}
	});
});

test("the User's write of /tmp/cpd-slack/mcp-policy.md is never a bare head", async () => {
	await withHarness((harness) => {
		const path = "/tmp/cpd-slack/mcp-policy.md";
		const id = `rail-${++sequence}`;
		announce(harness, id, "write");
		const row = makeRow("write", id, {}, harness.definitions.get("write"));
		const check = (label: string) => {
			const lines = drawn(row, 100);
			assert.ok(plain(lines[0]!).includes(path) || label === "announced", `${label}: ${JSON.stringify(lines.map(plain))}`);
			assert.ok(lines.length >= 2, `${label}: a bare head ${JSON.stringify(lines.map(plain))}`);
			assert.equal(closingOf(lines, label), "loader", label);
		};
		check("announced");
		row.updateArgs({ path });
		check("path streamed");
		// The content streams for seconds: the row is alive the whole time.
		for (let chunk = 1; chunk <= 3; chunk++) {
			row.updateArgs({ path, content: "# MCP policy\n".repeat(chunk * 40) });
			advance(1_000);
			check(`content chunk ${chunk}`);
		}
		row.setArgsComplete();
		check("arguments complete");
		harness.handlers.get("tool_execution_start")?.({ toolCallId: id, toolName: "write", args: { path } });
		row.markExecutionStarted();
		advance(500);
		check("running, no result");
	});
});

test("the closing row is not doubled when the first result arrives", async () => {
	await withHarness((harness) => {
		for (const kind of ordinary) {
			const id = `rail-${++sequence}`;
			announce(harness, id, kind.name);
			const row = makeRow(kind.name, id, kind.outcome.args, kind.definition(harness));
			row.setArgsComplete();
			harness.handlers.get("tool_execution_start")?.({ toolCallId: id, toolName: kind.name, args: kind.outcome.args });
			row.markExecutionStarted();
			const count = (label: string) => {
				const lines = drawn(row, 100);
				assert.equal(lines.filter((line) => plain(line).startsWith("\u2570")).length, 1, `${kind.key} / ${label}: ${JSON.stringify(lines.map(plain))}`);
			};
			count("started, no result");
			row.updateResult({ content: [], isError: false } as never, true);
			count("first (empty) result");
			row.updateResult({ ...(kind.outcome.partial ?? text("out")), isError: false } as never, true);
			count("partial result");
			row.updateResult({ ...kind.outcome.success, isError: false } as never, false);
			count("final result");
		}
	});
});

test("a failed or aborted row closes with its toggle", async () => {
	await withHarness((harness) => {
		for (const kind of KINDS) {
			for (const stage of ["failure", "aborted"] as const) {
				for (const expanded of [false, true]) {
					for (const width of WIDTHS) {
						const where = `${kind.key} / ${stage} / ${expanded ? "expanded" : "folded"} / ${width}`;
						const lines = drawn(rowAt(harness, kind, stage, expanded), width);
						assert.equal(closingOf(lines, where), "toggle", where);
						assert.match(plain(lines.find((line) => plain(line).startsWith("\u2570"))!), expanded ? /collapse/u : /expand/u, where);
					}
				}
			}
			// ...and it was never bare on the way there: the row that fails while its arguments stream is the loader before.
			assert.equal(closingOf(drawn(rowAt(harness, kind, "streaming", false), 100), `${kind.key} / before the failure`), "loader", kind.key);
		}
	});
});

test("a custom tool with its own result renderer closes with one of the five", async () => {
	await withHarness((harness) => {
		const author = KINDS.find((kind) => kind.key === "fetch@author")!;
		for (const stage of STAGES) {
			for (const expanded of [false, true]) {
				for (const width of WIDTHS) {
					const where = `fetch@author / ${stage} / ${expanded ? "expanded" : "folded"} / ${width}`;
					const form = closingOf(drawn(rowAt(harness, author, stage, expanded), width), where);
					assert.ok(EXPECTED[stage].includes(form), `${where}: closes with ${form}`);
				}
			}
		}
	});
});

test("ask_user_question closes with ╰󰬦 choose in colour 8 from the start of its execution until its result", async () => {
	await withHarness((harness) => {
		for (const stage of ["started", "empty partial", "partial"] as const) {
			for (const width of WIDTHS) {
				const where = `ask_user_question / ${stage} / ${width}`;
				const lines = drawn(rowAt(harness, ask, stage, false), width);
				assert.equal(closingOf(lines, where), "choose", where);
				const last = lines.at(-1)!;
				assert.equal(plain(last), "\u2570\u{F0B26} choose", where);
				assert.ok(!/\x1b\[(37|36|97)m/u.test(last), `${where}: the whole row is colour 8, not clickable: ${JSON.stringify(last)}`);
			}
		}
	});
});

test("ask_user_question closes with the loader while its arguments stream", async () => {
	await withHarness((harness) => {
		for (const stage of ["announced", "streaming", "complete"] as const) {
			for (const width of WIDTHS) {
				const where = `ask_user_question / ${stage} / ${width}`;
				assert.equal(closingOf(drawn(rowAt(harness, ask, stage, false), width), where), "loader", where);
			}
		}
	});
});

test("an answered ask_user_question closes with its fold toggle in 7", async () => {
	await withHarness((harness) => {
		// It was `choose` while the question was open; the answer replaces it, never both.
		assert.equal(closingOf(drawn(rowAt(harness, ask, "started", false), 100), "ask_user_question / open"), "choose");
		for (const expanded of [false, true]) {
			for (const width of WIDTHS) {
				const where = `ask_user_question / answered / ${expanded ? "expanded" : "folded"} / ${width}`;
				const lines = drawn(rowAt(harness, ask, "success", expanded), width);
				assert.equal(closingOf(lines, where), "toggle", where);
				assert.ok(!lines.some((line) => plain(line).includes("choose")), `${where}: no choose after the result`);
			}
		}
	});
});

test.after(restoreClocks);

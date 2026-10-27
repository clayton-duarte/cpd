/**
 * Chat balloons (docs/contract.md section 6.9, "Chat balloons"; Plan pi-theme): the pure drawing function
 * `drawBalloon` of pi/lib/balloon.ts, the theme tokens, and pi's own message components drawing them.
 */
import "./compact-tools/indicator-harness.ts";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { visibleWidth } from "@earendil-works/pi-tui";
import { AssistantMessageComponent, UserMessageComponent } from "@earendil-works/pi-coding-agent";
import { loadExtension, shutdown, type Harness } from "./compact-tools/indicator-harness.ts";

process.env.TZ = "Asia/Kolkata";
/** hs-d637: the one known instant every surface renders; in Asia/Kolkata (UTC+05:30) it is 03:11 the next day. */
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");

/** The time the agent balloon's bottom border carries: right after the tail, `│╭─HH:MM` then the rest of the dashes. */
const agentTail = (box: number): string =>
	box < 9
		? `\u2502\u256d${"\u2500".repeat(box - 3)}\u256f`
		: `\u2502\u256d\u250003:11${"\u2500".repeat(box - 9)}\u256f`;

type Side = "user" | "agent";
type Draw = (rows: string[], side: Side, width: number, at?: number) => string[];

// A variable specifier: the module does not exist yet, which is the red, and must not break the typecheck.
const BALLOON = "../lib/balloon.ts";
const draw = async (rows: string[], side: Side, width: number, at?: number): Promise<string[]> =>
	((await import(BALLOON)) as { drawBalloon: Draw }).drawBalloon(rows, side, width, at);

const SGR = /\x1b\[[0-9;]*m/g;
const OSC = /\x1b\][^\x07]*\x07/g;
const bare = (row: string) => row.replace(OSC, "").replace(SGR, "");
const plain = (rows: string[]) => rows.map(bare);

test("a user balloon is the template with the tail at the bottom right", async () => {
	const rows = await draw(["user prompt"], "user", 19);
	assert.deepEqual(plain(rows), [
		"    ╭─────────────╮",
		"    │ user prompt │",
		"    ╰────────────╮│",
		"                 ╰╯",
	]);
});

test("an agent balloon is the mirrored template with the tail at the bottom left", async () => {
	const rows = await draw(["agent reply"], "agent", 19, TIME_MS);
	assert.deepEqual(plain(rows), [
		"",
		"╭─────────────╮",
		"│ agent reply │",
		agentTail(15),
		"╰╯",
	]);
});

test("the agent balloon omits the time when the corner is too small for it", async () => {
	const rows = await draw(["one"], "agent", 19, TIME_MS);
	assert.deepEqual(plain(rows), ["", "╭─────╮", "│ one │", "│╭────╯", "╰╯"]);
});

test("the balloon is the longest row plus two padding columns and two borders wide", async () => {
	const rows = ["fix the build", "then ship it"];
	const user = plain(await draw(rows, "user", 24));
	assert.deepEqual(user, [
		"       ╭───────────────╮",
		"       │ fix the build │",
		"       │ then ship it  │",
		"       ╰──────────────╮│",
		"                      ╰╯",
	]);
	const agent = plain(await draw(rows, "agent", 24));
	assert.deepEqual(agent, [
		"",
		"╭───────────────╮",
		"│ fix the build │",
		"│ then ship it  │",
		"│╭──────────────╯",
		"╰╯",
	]);
	// A wider pane does not widen the balloon.
	assert.deepEqual(plain(await draw(rows, "agent", 80)), agent);
});

test("a user balloon ends in the pane's last column and an agent balloon starts in the first column under a blank row", async () => {
	for (const width of [15, 24, 60]) {
		const user = plain(await draw(["hello"], "user", width));
		assert.equal(user.length, 4);
		for (const row of user) assert.equal(row.length, width, `user row ${JSON.stringify(row)} ends in column ${width}`);
		assert.equal(user[0]!.trim().length, 9);
		assert.equal(user[0]![width - 1], "╮");
		const agent = plain(await draw(["hello"], "agent", width));
		assert.equal(agent[0], "");
		assert.equal(agent[1]![0], "╭");
		assert.equal(agent[2]![0], "│");
		assert.equal(agent[4]![0], "╰");
	}
});

test("every row of a multi-line balloon keeps the right wall in one column", async () => {
	const rows = ["a", "a longer line here", "mid", ""];
	const user = plain(await draw(rows, "user", 40));
	const wall = user[0]!.length - 1;
	assert.equal(user.length, rows.length + 3);
	for (const row of user.slice(0, 1 + rows.length + 1)) assert.equal(row[wall], row.trim().at(-1), row);
	for (const row of user.slice(1, 1 + rows.length)) assert.equal(row[wall], "│", row);
	const agent = plain(await draw(rows, "agent", 40));
	assert.equal(agent[0], "", "the agent's blank top-margin row");
	const bodies = agent.slice(2, 2 + rows.length);
	for (const row of bodies) assert.equal(row.length, 22, row);
	for (const row of bodies) assert.ok(row.startsWith("│") && row.endsWith("│"), row);
});

test("borders and tails are colour 8 and the text is not recoloured", async () => {
	const styled = "\x1b[35mhi\x1b[39m there";
	for (const side of ["user", "agent"] as const) {
		const rows = await draw([styled], side, 30);
		assert.ok(rows.some((row) => row.includes(styled)), `${side}: the text row keeps its own codes`);
		const joined = rows.join("\n");
		assert.doesNotMatch(joined, /\x1b\[(?:[0-9;]*;)?(?:2|7)m/, "no faint and no reverse");
		assert.doesNotMatch(joined, /\x1b\[[34]8;2;/, "no RGB");
		for (const row of rows) {
			let fg: string | undefined;
			for (const part of row.split(/(\x1b\[[0-9;]*m)/)) {
				if (part.startsWith("\x1b[")) {
					for (const code of part.slice(2, -1).split(";")) {
						if (code === "0" || code === "" || code === "39") fg = undefined;
						else if (code === "90") fg = "90";
						else if (/^(3[0-7]|9[1-7])$/.test(code)) fg = code;
					}
					continue;
				}
				for (const ch of part) {
					if ("╭╮╰╯│─".includes(ch)) assert.equal(fg, "90", `${side}: ${ch} in ${JSON.stringify(row)} is colour 8`);
				}
			}
		}
	}
});

test("wide characters and styled text keep the wall aligned", async () => {
	const rows = ["漢字漢字", "\x1b[1mbold\x1b[22m text", "ab"];
	for (const side of ["user", "agent"] as const) {
		const out = await draw(rows, side, 30);
		const top = side === "agent" ? 1 : 0;
		const body = out.slice(top + 1, top + 1 + rows.length);
		const widths = body.map(visibleWidth);
		assert.equal(new Set(widths).size, 1, `${side}: ${widths}`);
		for (const row of body) assert.ok(bare(row).trimEnd().endsWith("│"), row);
		assert.equal(visibleWidth(out[top]!), side === "user" ? 30 : 13, "longest row 9 columns + 4; the user's row ends in the pane's last column");
	}
});

test("no row is wider than the pane and a very narrow pane draws no balloon", async () => {
	for (const side of ["user", "agent"] as const) {
		for (const width of [9, 12, 30]) {
			const inner = Math.min(width - 8, 4);
			const out = await draw(["x".repeat(inner)], side, width);
			for (const row of out) assert.ok(visibleWidth(row) <= width, `${side} ${width}: ${JSON.stringify(row)}`);
			assert.ok(out.length > 1, "a balloon is drawn from width 9");
		}
		const rows = ["ab", "cd"];
		for (const width of [1, 4, 8]) assert.deepEqual(await draw(rows, side, width), rows, `${side} ${width}`);
	}
});

test("the dango theme no longer inverts the user's input", () => {
	const theme = JSON.parse(readFileSync(new URL("../themes/dango.json", import.meta.url), "utf8")) as { colors: Record<string, unknown> };
	assert.equal(theme.colors.userMessageBg, "");
	assert.equal(theme.colors.userMessageText, 7);
});

// ---- pi's own components, under fold status (the harness default): the earlier geometry (one column of right margin
// for the user, no blank row above the agent's) holds there; test/chat-margins.test.ts and balloon-margins.test.ts
// pin the compact toggle rows ----

const ZONE_A = "\x1b]133;A\x07";
const ZONE_BC = "\x1b]133;B\x07\x1b]133;C\x07";
const BOX = /[╭╮╰╯│]/;

async function withHarness<T>(body: (harness: Harness) => Promise<T> | T): Promise<T> {
	const harness = await loadExtension({}, { tui: true });
	try {
		return await body(harness);
	} finally {
		shutdown(harness);
	}
}

const assistant = (content: unknown[], extra: object = {}) =>
	new AssistantMessageComponent({ role: "assistant", content, stopReason: "stop", timestamp: TIME_MS, ...extra } as never);

test("pi's user message component draws a user balloon with its OSC 133 zone marks", async () => {
	await withHarness(() => {
		const rows = new UserMessageComponent("user prompt").render(40);
		assert.deepEqual(plain(rows), [
			" ".repeat(24) + "╭─────────────╮",
			" ".repeat(24) + "│ user prompt │",
			" ".repeat(24) + "╰────────────╮│",
			" ".repeat(24) + "             ╰╯",
		]);
		assert.ok(rows[0]!.startsWith(ZONE_A), "the zone start is on the first row");
		assert.ok(rows.at(-1)!.includes(ZONE_BC), "the zone end is on the last row");
		for (const row of rows) assert.ok(visibleWidth(row) <= 40);
	});
});

test("pi's assistant message component draws one agent balloon per text block", async () => {
	await withHarness(() => {
		const rows = assistant([{ type: "text", text: "one" }, { type: "text", text: "two" }]).render(40);
		assert.deepEqual(plain(rows), [
			"╭─────╮", "│ one │", "│╭────╯", "╰╯",
			"╭─────╮", "│ two │", "│╭────╯", "╰╯",
		]);
		assert.ok(rows[0]!.startsWith(ZONE_A));
		assert.ok(rows.at(-1)!.includes(ZONE_BC));
		// The text wraps at the pane width minus eight (margins, borders and paddings).
		const wrapped = plain(assistant([{ type: "text", text: "alpha beta gamma delta" }]).render(14));
		for (const row of wrapped) assert.ok(row.length <= 11, row);
		assert.ok(wrapped.slice(1, -2).every((row) => row.startsWith("│ ") && row.endsWith(" │")), wrapped.join("\n"));
	});
});

test("thinking, tool calls and errors of an assistant message stay outside the balloon", async () => {
	await withHarness(() => {
		const truncated = plain(assistant(
			[{ type: "thinking", thinking: "pondering" }, { type: "text", text: "answer" }],
			{ stopReason: "length" },
		).render(40));
		const boxed = truncated.filter((row) => BOX.test(row));
		assert.equal(boxed.length, 4, truncated.join("\n"));
		assert.ok(boxed.some((row) => row.includes("answer")));
		for (const row of truncated.filter((r) => r.includes("pondering") || r.includes("Response was truncated"))) {
			assert.doesNotMatch(row, BOX, row);
		}
		assert.ok(truncated.some((row) => row.includes("pondering")));
		assert.ok(truncated.some((row) => row.includes("Response was truncated")));
		const failed = plain(assistant([{ type: "text", text: "partial" }], { stopReason: "error", errorMessage: "boom" }).render(40));
		assert.equal(failed.filter((row) => BOX.test(row)).length, 4, failed.join("\n"));
		const error = failed.find((row) => row.includes("Error: boom"));
		assert.ok(error, failed.join("\n"));
		assert.doesNotMatch(error, BOX);
	});
});

test("an assistant message with no text draws no balloon", async () => {
	await withHarness(() => {
		const control = plain(assistant([{ type: "text", text: "hi" }]).render(40));
		assert.ok(control.some((row) => BOX.test(row)), "a message with text is boxed");
		const thinking = plain(assistant([{ type: "thinking", thinking: "pondering" }]).render(40));
		assert.ok(thinking.some((row) => row.includes("pondering")));
		const calls = plain(assistant([{ type: "toolCall", id: "c1", name: "read", arguments: {} }]).render(40));
		for (const row of [...thinking, ...calls]) assert.doesNotMatch(row, BOX, row);
		const blank = plain(assistant([{ type: "text", text: "  \n " }]).render(40));
		for (const row of blank) assert.doesNotMatch(row, BOX, row);
	});
});

test("a mention in the text is painted inside the balloon", async () => {
	const key = Symbol.for("cpd.mentions.painter");
	const holder = globalThis as unknown as Record<symbol, ((line: string) => string) | undefined>;
	// A row that already holds a border was boxed before it was painted: the painter marks it.
	holder[key] = (line) => (BOX.test(bare(line)) ? "PAINTED AFTER BOXING" : line.replace("abc12345", "\x1b[35mabc12345\x1b[39m"));
	try {
		await withHarness(() => {
			const rows = assistant([{ type: "text", text: "ping abc12345" }]).render(40);
			const text = rows.find((row) => row.includes("abc12345"));
			assert.ok(text, rows.join("\n"));
			assert.ok(text.includes("\x1b[35mabc12345\x1b[39m"), "the mention is magenta");
			assert.equal(bare(text), "│ ping abc12345 │");
			assert.ok(!rows.join("").includes("PAINTED AFTER BOXING"));
			assert.equal(rows.filter((row) => BOX.test(bare(row))).length, 4);
		});
	} finally {
		delete holder[key];
	}
});

test("silent mode draws the answer as a balloon", async () => {
	await withHarness(async (harness) => {
		const ctx = { cwd: "/project", mode: "tui", isIdle: () => false, ui: { notify: () => {} } };
		await harness.commands.get("silent")!.handler("on", ctx);
		try {
			const final = assistant([{ type: "thinking", thinking: "pondering" }, { type: "text", text: "answer" }], { stopReason: "length" });
			assert.deepEqual(plain(final.render(40)), ["╭────────╮", "│ answer │", agentTail(10), "╰╯"]);
			const handoff = assistant([{ type: "text", text: "Before call" }, { type: "toolCall", id: "c1", name: "read", arguments: {} }]);
			assert.deepEqual(handoff.render(40), [], "an intermediate turn is still hidden");
		} finally {
			await harness.commands.get("silent")!.handler("off", ctx);
		}
	});
});

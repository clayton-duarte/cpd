/**
 * Chat balloon margins (docs/contract.md section 6.9, "Chat balloons"; Plans pi-theme and chat-margins): the agent's
 * balloon starts in column 0 (the root pad supplies the left cell) with one blank row above it; the user's ends in the
 * pane's last column; the agent's width is min(pane - 4, longest row + 4) and its text wraps at pane - 8, the user's
 * min(pane - 3, longest row + 4) and pane - 7; 1 padding column each side, none above or below.
 */
import "./compact-tools/indicator-harness.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { AssistantMessageComponent, UserMessageComponent } from "@earendil-works/pi-coding-agent";
import { loadExtension, shutdown } from "./compact-tools/indicator-harness.ts";

type Side = "user" | "agent";
type Draw = (rows: string[], side: Side, width: number) => string[];

const BALLOON = "../lib/balloon.ts";
const draw = async (rows: string[], side: Side, width: number): Promise<string[]> =>
	((await import(BALLOON)) as { drawBalloon: Draw }).drawBalloon(rows, side, width);

const SGR = /\x1b\[[0-9;]*m/g;
const OSC = /\x1b\][^\x07]*\x07/g;
const plain = (rows: string[]) => rows.map((row) => row.replace(OSC, "").replace(SGR, ""));

test("an agent balloon starts at the padded content edge and leaves at least three columns on its right", async () => {
	const out = plain(await draw(["agent reply"], "agent", 40));
	assert.deepEqual(out, [
		"",
		"╭─────────────╮",
		"│ agent reply │",
		"│╭────────────╯",
		"╰╯",
	]);
	const wide = plain(await draw(["x".repeat(60)], "agent", 20));
	for (const row of wide) assert.ok(row.length <= 16, `three margin columns on the right: ${JSON.stringify(row)}`);
	assert.ok(wide[1]!.startsWith("╭"));
});

test("a user balloon has no outer right margin and at least three columns on its left", async () => {
	const out = plain(await draw(["user prompt"], "user", 40));
	const pad = " ".repeat(25);
	assert.deepEqual(out, [
		pad + "╭─────────────╮",
		pad + "│ user prompt │",
		pad + "╰────────────╮│",
		pad + "             ╰╯",
	]);
	const wide = plain(await draw(["x".repeat(60)], "user", 20));
	for (const row of wide) {
		assert.equal(row.length, 20, JSON.stringify(row));
		assert.ok(row.startsWith("   "), `three margin columns on the left: ${JSON.stringify(row)}`);
		assert.ok(!row.endsWith(" "), `no outer right margin: ${JSON.stringify(row)}`);
	}
});

test("a balloon spans its text up to the pane width minus its far gap (four for the agent, three for the user)", async () => {
	const top = (rows: string[]) => rows.find((row) => row.trim() !== "")!.trim();
	for (const side of ["user", "agent"] as const) {
		const gap = side === "user" ? 3 : 4;
		const short = plain(await draw(["hello"], side, 60));
		assert.equal(top(short).length, 9, `${side}: longest row 5 + 4`);
		for (const width of [12, 20, 30]) {
			const out = plain(await draw(["x".repeat(width - gap - 4)], side, width));
			assert.equal(top(out).length, width - gap, `${side} ${width}: fills pane minus ${gap}`);
			const over = plain(await draw(["x".repeat(width)], side, width));
			assert.equal(top(over).length, width - gap, `${side} ${width}: never wider than pane minus ${gap}`);
		}
	}
});

test("text wraps at the pane width minus eight (the agent) or minus seven (the user)", async () => {
	const harness = await loadExtension({ style: "compact", fold: "toggle" }, { tui: true, defaults: true });
	try {
		const text = "alpha beta gamma delta epsilon zeta";
		const width = 20;
		const agent = plain(
			new AssistantMessageComponent({ role: "assistant", content: [{ type: "text", text }], stopReason: "stop" } as never).render(width),
		);
		const user = plain(new UserMessageComponent(text).render(width));
		for (const [rows, limit] of [[agent, width - 8], [user, width - 7]] as const) {
			const body = rows.filter((row) => row.includes("│ ") && /[a-z]/.test(row));
			assert.ok(body.length >= 3, rows.join("\n"));
			for (const row of body) {
				const inner = row.slice(row.indexOf("│") + 2, row.lastIndexOf("│") - 1).trimEnd();
				assert.ok(inner.length <= limit, `${JSON.stringify(inner)} is at most ${limit} columns`);
			}
			assert.ok(body.some((row) => row.includes("alpha beta")), "words fill the columns");
		}
	} finally {
		shutdown(harness);
	}
});

test("a balloon has one padding column on each side and none above or below", async () => {
	for (const side of ["user", "agent"] as const) {
		const drawn = plain(await draw(["ab", "abcd"], side, 40)).map((row) => row.trim());
		// The agent's balloon is preceded by its one blank top-margin row; the balloon itself has none inside.
		if (side === "agent") assert.equal(drawn[0], "", "agent: one blank row of top margin");
		const out = side === "agent" ? drawn.slice(1) : drawn;
		assert.deepEqual(out.slice(0, 3), ["╭──────╮", "│ ab   │", "│ abcd │"], side);
		assert.equal(out.length, 5, `${side}: top, two text rows, two tail rows`);
		assert.ok(out.every((row) => row !== ""), `${side}: no blank row in the balloon`);
		assert.ok(!out.some((row) => /^│\s+│$/.test(row)), `${side}: no blank inner row`);
	}
});

test("the user's tail starts directly under the last text row", async () => {
	const out = plain(await draw(["one", "two words"], "user", 30));
	const last = out.findIndex((row) => row.includes("two words"));
	const left = out[last]!.indexOf("│");
	assert.equal(out[last + 1], " ".repeat(left) + "╰" + "─".repeat(10) + "╮│");
	assert.equal(out[last + 2], " ".repeat(left + 11) + "╰╯");
	assert.equal(out.length, last + 3);
});

test("the agent's tail starts directly under the last text row", async () => {
	const out = plain(await draw(["one", "two words"], "agent", 30));
	const last = out.findIndex((row) => row.includes("two words"));
	assert.equal(out[last + 1], "│╭" + "─".repeat(10) + "╯");
	assert.equal(out[last + 2], "╰╯");
	assert.equal(out.length, last + 3);
});

test("no blank row sits below a balloon, nor above the user's, in pi's components", async () => {
	const harness = await loadExtension({ style: "compact", fold: "toggle" }, { tui: true, defaults: true });
	try {
		const user = plain(new UserMessageComponent("user prompt").render(40));
		assert.equal(user.length, 4, user.join("\n"));
		assert.ok(user[0]!.includes("╭") && user.at(-1)!.includes("╰╯"), user.join("\n"));
		assert.equal(user[0]![25], "╭");
		assert.ok(user.every((row) => row.trim() !== ""), "no blank row in the user balloon");
		const agent = plain(
			new AssistantMessageComponent({ role: "assistant", content: [{ type: "text", text: "agent reply" }], stopReason: "stop" } as never).render(40),
		);
		assert.deepEqual(agent, ["", "╭─────────────╮", "│ agent reply │", "│╭────────────╯", "╰╯"]);
	} finally {
		shutdown(harness);
	}
});

/**
 * Chat balloon margins (docs/contract.md section 6.9, "Chat balloons"; Plan pi-theme): the agent's balloon has
 * 1 margin column on the left and at least 3 on the right, the user's the opposite; the width is
 * min(pane - 4, longest row + 4); text wraps at pane - 8; 1 padding column each side, none above or below.
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

test("an agent balloon has one margin column on its left and at least three on its right", async () => {
	const out = plain(await draw(["agent reply"], "agent", 40));
	assert.deepEqual(out, [
		" ╭─────────────╮",
		" │ agent reply │",
		" │╭────────────╯",
		" ╰╯",
	]);
	const wide = plain(await draw(["x".repeat(60)], "agent", 20));
	for (const row of wide) assert.ok(row.length <= 17, `three margin columns on the right: ${JSON.stringify(row)}`);
	assert.ok(wide[0]!.startsWith(" ╭"));
});

test("a user balloon has one margin column on its right and at least three on its left", async () => {
	const out = plain(await draw(["user prompt"], "user", 40));
	const pad = " ".repeat(24);
	assert.deepEqual(out, [
		pad + "╭─────────────╮ ",
		pad + "│ user prompt │ ",
		pad + "╰────────────╮│ ",
		pad + "             ╰╯ ",
	]);
	const wide = plain(await draw(["x".repeat(60)], "user", 20));
	for (const row of wide) {
		assert.equal(row.length, 20, JSON.stringify(row));
		assert.ok(row.startsWith("   "), `three margin columns on the left: ${JSON.stringify(row)}`);
		assert.ok(row.endsWith(" ") && !row.endsWith("  "), `one margin column on the right: ${JSON.stringify(row)}`);
	}
});

test("a balloon spans its text up to the pane width minus four", async () => {
	for (const side of ["user", "agent"] as const) {
		const short = plain(await draw(["hello"], side, 60));
		assert.equal(short[0]!.trim().length, 9, `${side}: longest row 5 + 4`);
		for (const width of [12, 20, 30]) {
			const out = plain(await draw(["x".repeat(width - 8)], side, width));
			assert.equal(out[0]!.trim().length, width - 4, `${side} ${width}: fills pane minus four`);
			const over = plain(await draw(["x".repeat(width)], side, width));
			assert.equal(over[0]!.trim().length, width - 4, `${side} ${width}: never wider than pane minus four`);
		}
	}
});

test("text wraps at the pane width minus eight", async () => {
	const harness = await loadExtension({}, { tui: true });
	try {
		const text = "alpha beta gamma delta epsilon zeta";
		const width = 20;
		const agent = plain(
			new AssistantMessageComponent({ role: "assistant", content: [{ type: "text", text }], stopReason: "stop" } as never).render(width),
		);
		const user = plain(new UserMessageComponent(text).render(width));
		for (const rows of [agent, user]) {
			const body = rows.filter((row) => row.includes("│ ") && /[a-z]/.test(row));
			assert.ok(body.length >= 3, rows.join("\n"));
			for (const row of body) {
				const inner = row.slice(row.indexOf("│") + 2, row.lastIndexOf("│") - 1).trimEnd();
				assert.ok(inner.length <= width - 8, `${JSON.stringify(inner)} is at most ${width - 8} columns`);
			}
			assert.ok(body.some((row) => row.includes("alpha beta")), "words fill the 12 columns");
		}
	} finally {
		shutdown(harness);
	}
});

test("a balloon has one padding column on each side and none above or below", async () => {
	for (const side of ["user", "agent"] as const) {
		const out = plain(await draw(["ab", "abcd"], side, 40)).map((row) => row.trim());
		assert.deepEqual(out.slice(0, 3), ["╭──────╮", "│ ab   │", "│ abcd │"], side);
		assert.equal(out.length, 5, `${side}: top, two text rows, two tail rows`);
		assert.ok(out.every((row) => row !== ""), `${side}: no blank row`);
		assert.ok(!out.some((row) => /^│\s+│$/.test(row)), `${side}: no blank inner row`);
	}
});

test("the user's tail starts directly under the last text row", async () => {
	const out = plain(await draw(["one", "two words"], "user", 30));
	const last = out.findIndex((row) => row.includes("two words"));
	const left = out[last]!.indexOf("│");
	assert.equal(out[last + 1], " ".repeat(left) + "╰" + "─".repeat(10) + "╮│ ");
	assert.equal(out[last + 2], " ".repeat(left + 11) + "╰╯ ");
	assert.equal(out.length, last + 3);
});

test("the agent's tail starts directly under the last text row", async () => {
	const out = plain(await draw(["one", "two words"], "agent", 30));
	const last = out.findIndex((row) => row.includes("two words"));
	assert.equal(out[last + 1], " │╭" + "─".repeat(10) + "╯");
	assert.equal(out[last + 2], " ╰╯");
	assert.equal(out.length, last + 3);
});

test("no blank row sits above or below a balloon in pi's components", async () => {
	const harness = await loadExtension({}, { tui: true });
	try {
		const user = plain(new UserMessageComponent("user prompt").render(40));
		assert.equal(user.length, 4, user.join("\n"));
		assert.ok(user[0]!.includes("╭") && user.at(-1)!.includes("╰╯"), user.join("\n"));
		assert.equal(user[0]![25], "╭");
		assert.ok(user.every((row) => row.trim() !== ""), "no blank row in the user balloon");
		const agent = plain(
			new AssistantMessageComponent({ role: "assistant", content: [{ type: "text", text: "agent reply" }], stopReason: "stop" } as never).render(40),
		);
		assert.deepEqual(agent, [" ╭─────────────╮", " │ agent reply │", " │╭────────────╯", " ╰╯"]);
	} finally {
		shutdown(harness);
	}
});

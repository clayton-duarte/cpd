/**
 * Silent mode's activity line is drawn in palette indices 0-15 only, whatever the
 * thinking level or the theme: neither changes a frame.
 */
import "./pinned-themes.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { timerClock } from "./indicator-harness.ts";
import {
	BranchSummaryMessageComponent,
	CompactionSummaryMessageComponent,
	initTheme,
	UserMessageComponent,
} from "@earendil-works/pi-coding-agent";
import { Spacer } from "@earendil-works/pi-tui";
import { renderActivityDots } from "../../compact-tools/extensions/compact-tools-activity.ts";
import { loadExtension, restoreClocks, shutdown } from "./indicator-harness.ts";

const colorsOf = (line: string) => [...line.matchAll(/\x1b\[(\d+)m━/gu)].map((match) => match[1]);
const silentState = () =>
	(globalThis as Record<symbol, { active: boolean; enabled: boolean }>)[Symbol.for("pi-compact-tools.silent.state")]!;

test("the line under a silent prompt is the same in every thinking level and theme", async () => {
	initTheme("dark", false);
	const harness = await loadExtension({ style: "compact", mode: "silent" }, { tui: true, idle: false });
	try {
		harness.handlers.get("agent_start")!({});
		harness.chat.children = [new UserMessageComponent("Look around")];
		// The line attaches to the latest prompt once a render pass settles.
		harness.chat.render(80);
		await Promise.resolve();
		const activity = () => harness.chat.render(80).find((line) => line.includes("━")) ?? "";
		/** One whole breath: the light is back where it was, so only its color can differ. */
		const breath = () => timerClock.tick(80 * 26);
		timerClock.tick(80 * 7);
		const medium = activity();
		assert.ok(colorsOf(medium).length > 0, "the line is drawn");

		harness.setThinkingLevel("high");
		breath();
		const high = activity();
		assert.equal(high, medium, "the thinking level does not paint the line");
		harness.setThinkingLevel("medium");
		breath();
		assert.equal(activity(), medium);

		initTheme("light", false);
		assert.equal(activity(), medium, "the theme does not paint the line");
		initTheme("dark", false);
	} finally {
		initTheme("dark", false);
		silentState().enabled = false;
		shutdown(harness);
	}
});

/**
 * Pi compacts between turns and writes the summary straight into the chat, after
 * a spacer of its own. In silent mode that row would land under the prompt the
 * line animates beneath, pushing the line up and away from the editor.
 */
test("a summary Pi writes mid-turn leaves the silent line where it was, at the foot of the chat", async () => {
	initTheme("dark", false);
	const harness = await loadExtension({ style: "compact", mode: "silent" }, { tui: true, idle: false });
	try {
		harness.handlers.get("agent_start")!({});
		const prompt = new UserMessageComponent("Look around");
		harness.chat.children = [prompt];
		// The line attaches to the latest prompt once a render pass settles.
		harness.chat.render(80);
		await Promise.resolve();
		timerClock.tick(80 * 7);
		const settled = harness.chat.render(80);
		assert.ok(settled.at(-1)?.includes("━"), "the line is the chat's last row");

		for (const summary of [
			new CompactionSummaryMessageComponent({ role: "compactionSummary", tokensBefore: 257_586, summary: "## Goal\nLook around" } as never),
			new BranchSummaryMessageComponent({ role: "branchSummary", summary: "## Goal\nLook around" } as never),
		]) {
			harness.chat.children = [prompt, new Spacer(1), summary];
			assert.deepEqual(harness.chat.render(80), settled, `${summary.constructor.name} and its spacer are hidden`);
		}

		// Out of silent mode the summary is the reader's again, spacer included.
		silentState().enabled = false;
		const visible = harness.chat.render(80);
		assert.ok(visible.length > settled.length, "the summary comes back");
		assert.ok(visible.some((line) => line.includes("[compaction]")) || visible.some((line) => line.includes("Branch summary")));
	} finally {
		silentState().enabled = false;
		shutdown(harness);
	}
});

for (const style of ["compact", "claude", "codex"] as const) test(`silent file-read animation survives low-level run end and a continuation (${style})`, async () => {
	const harness = await loadExtension({ style, mode: "silent" }, { tui: true, idle: false });
	try {
		harness.handlers.get("agent_start")!({});
		harness.chat.children = [new UserMessageComponent("Read several files")];
		harness.chat.render(80);
		await Promise.resolve();
		const activity = () => harness.chat.render(80).find((line) => line.includes("━"));
		harness.handlers.get("tool_execution_start")!({ toolCallId: "read-a", toolName: "read", args: { path: "a.ts" } });
		timerClock.tick(80 * 7);
		const reading = activity();
		assert.ok(reading);
		harness.handlers.get("agent_end")!({ messages: [] });
		timerClock.tick(80 * 7);
		assert.ok(activity(), "agent_end is not final settlement; do not remove the activity");
		assert.notEqual(activity(), reading);
		harness.handlers.get("turn_start")!({});
		harness.handlers.get("tool_execution_start")!({ toolCallId: "read-b", toolName: "read", args: { path: "b.ts" } });
		const resumed = activity();
		timerClock.tick(80 * 7);
		assert.ok(activity());
		assert.notEqual(activity(), resumed, "continuation must keep advancing frames");
		harness.handlers.get("agent_settled")!({});
		assert.equal(activity(), undefined, "final settlement removes the activity");
	} finally { silentState().enabled = false; shutdown(harness); }
});

test.after(() => {
	initTheme("dark", false);
	restoreClocks();
});

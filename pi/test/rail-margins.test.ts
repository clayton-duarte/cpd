/**
 * The rail template (docs/contract.md 6.1.1, 6.9): in a pane W wide a rail fold's body row is the margin (col 0),
 * the rail (col 1), a blank (col 2), content in cols 3..W-4 (W-6 cells, a box's content width), three blank columns.
 * So no row is wider than W-3 and text wraps at W-6 cells. The Thinking fold has no blank rail row under its head.
 */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { CustomMessageComponent } from "../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/custom-message.js";
import { theme } from "../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { Markdown } from "../../node_modules/@earendil-works/pi-tui/dist/components/markdown.js";
import { getMarkdownTheme } from "../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { initTheme, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { ThinkingCycleController } from "../compact-tools/extensions/compact-tools-thinking.ts";
import { Rendering } from "../lib/render.ts";
import { advance, announce, CASES, loadExtension, makeRow, restoreClocks, shutdown, text } from "./compact-tools/indicator-harness.ts";

initTheme("dark", false);
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "rail-margins-agent-"));
test.after(() => { restoreClocks(); rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }); });

const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");
const WIDTHS = [40, 80];
// No spaces: a row is filled to the very cell where the component wraps or cuts.
const long = "abcdefghij".repeat(30);

/** Every body row (a `│` rail row with text) is ` │ ` + text, and its last cell is at column W-4 at most. */
function checkBody(rows: string[], width: number, where: string): void {
	const body = rows.map(plain).filter((r) => /^ ?│/u.test(r) && r.slice(2).trim() !== "");
	assert.ok(body.length >= 2, `body rows in ${where}: ${JSON.stringify(rows.map(plain))}`);
	for (const r of body) {
		assert.ok(r.startsWith(" │ "), `margin, rail, blank: ${JSON.stringify(r)} in ${where}`);
		assert.ok(r.trimEnd().length <= width - 3, `ends at column W-4 (${width}): ${JSON.stringify(r)} (${r.trimEnd().length}) in ${where}`);
	}
	assert.equal(Math.max(...body.map((r) => r.trimEnd().length)), width - 3, `text fills up to column W-4 (${width}) in ${where}: ${JSON.stringify(body)}`);
}

test("a cpd message fold wraps its body at the content width of a box", () => {
	for (const width of WIDTHS)
		for (const customType of ["cpd", "cpd-plan-summary"]) {
			const message = { role: "custom" as const, timestamp: 0, customType, content: long, display: true, details: { review: "r1" } };
			const component = new CustomMessageComponent(message, new Rendering().render, undefined, 1);
			component.setExpanded(true);
			const rows = component.render(width).filter((l: string) => l.trim() !== "");
			checkBody(rows, width, `${customType} at ${width}`);
		}
});

test("a tool row fold keeps three blank columns after its text", async () => {
	for (const width of WIDTHS) {
		const harness = await loadExtension({ style: "compact", fold: "toggle" }, { defaults: true });
		try {
			const id = `rm-${width}`;
			const outcome = CASES.bash!;
			announce(harness, id, "bash");
			const row = makeRow("bash", id, outcome.args, harness.definitions.get("bash"));
			row.setArgsComplete();
			harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: "bash", args: outcome.args });
			row.markExecutionStarted();
			advance(300);
			harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: false });
			row.updateResult({ ...text(`${long}\n${long}`), isError: false } as never, false);
			row.setExpanded(true);
			const rows = row.render(width).filter((l) => plain(l).trim() !== "");
			checkBody(rows, width, `bash at ${width}`);
		} finally {
			shutdown(harness);
		}
	}
});

const NERD = { expand: "\u{F004B}", collapse: "\u{F0063}" };
const SOURCE = `Weighing the options.\n\n${long}\n\nKeep the cache.`;

/** The expanded block as the host draws it: Pi's Markdown component (paddingX = outputPad 1) calling the transformer, at pane width W. */
function expandedThinking(width: number): string[] {
	let transform: ((markdown: string, context: unknown) => string) | undefined;
	let input: ((data: string) => unknown) | undefined;
	const pi = { on: () => {}, registerMarkdownTransformer: (h: typeof transform) => { transform = h; } } as unknown as ExtensionAPI;
	const controller = new ThinkingCycleController(pi, () => true, () => NERD);
	controller.bind({
		ui: { theme, setHiddenThinkingLabel: () => {}, onTerminalInput: (h: typeof input) => { input = h; return () => {}; } },
	} as unknown as ExtensionContext);
	try {
		const draw = () => new Markdown(SOURCE, 1, 0, getMarkdownTheme(), undefined, {
			transform: (markdown: string, availableWidth: number) =>
				transform!(markdown, { messageType: "assistant-thinking", isStreaming: false, availableWidth }),
		}).render(width);
		draw();
		input!("\x14");
		return draw();
	} finally {
		controller.dispose();
	}
}

test("the Thinking fold has no blank rail row between its head and its first body row", () => {
	for (const width of WIDTHS) {
		const rows = expandedThinking(width).map(plain);
		const head = rows.findIndex((r) => r.includes("Thinking…"));
		assert.ok(head >= 0, JSON.stringify(rows));
		assert.match(rows[head + 1]!, /^ ?│ \S/u, `the row under the head is the first body row at ${width}: ${JSON.stringify(rows)}`);
		assert.ok(rows[head + 1]!.includes("Weighing the options."), JSON.stringify(rows));
	}
});

test("the Thinking fold wraps its body at the content width of a box", () => {
	for (const width of WIDTHS) {
		const rows = expandedThinking(width).filter((l) => plain(l).trim() !== "");
		checkBody(rows, width, `Thinking at ${width}`);
	}
});

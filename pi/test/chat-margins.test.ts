/** Chat margins (docs/contract.md, Plan chat-margins): exact rows of the five tweaks, ANSI stripped, at widths 40 and 80. */
import "./compact-tools/indicator-harness.ts";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { AssistantMessageComponent, UserMessageComponent, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { CustomEditor } from "../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/custom-editor.js";
import { theme } from "../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { ThinkingCycleController } from "../compact-tools/extensions/compact-tools-thinking.ts";
import { advance, announce, CASES, loadExtension, makeRow, restoreClocks, shutdown, text } from "./compact-tools/indicator-harness.ts";

process.env.TZ = "Asia/Kolkata";
const agentDir = mkdtempSync(join(tmpdir(), "chat-margins-agent-"));
process.env.PI_CODING_AGENT_DIR = agentDir;
test.after(() => { restoreClocks(); rmSync(agentDir, { recursive: true, force: true }); });

const plain = (line: string) => line.replace(/\x1b\][^\x07]*\x07/gu, "").replace(/\x1b_pi:\/?(?:fc|c)\x07/gu, "").replace(/\x1b\[[0-9;]*m/gu, "");
const WIDTHS = [40, 80];
const COLLAPSE = "\u{F0B2C}";
const EXPAND = "\u{F0B26}";
const NERD = { expand: EXPAND, collapse: COLLAPSE };

test("chat margins: the input box spans the pane with no margin column", async () => {
	const handlers = new Map<string, Array<(event: unknown, ctx?: unknown) => void>>();
	const compactTools = (await import("../compact-tools/extensions/compact-tools.ts")).default;
	compactTools({
		on: (name: string, h: (event: unknown, ctx?: unknown) => void) => handlers.set(name, [...(handlers.get(name) ?? []), h]),
		registerTool: () => {}, registerMarkdownTransformer: () => {}, registerCommand: () => {}, registerShortcut: () => {},
		getThinkingLevel: () => "medium",
	} as unknown as ExtensionAPI);
	const tui = { terminal: { rows: 40, columns: 80 }, requestRender: () => {} };
	const ui = new Proxy({} as Record<string, unknown>, {
		get: (_t, key) => key === "theme" ? theme : key === "setWidget" ? (_k: string, f: unknown) => { if (typeof f === "function") f(tui); } : () => () => {},
	});
	const ctx = { cwd: "/project", mode: "tui", isProjectTrusted: () => false, isIdle: () => true, ui } as unknown as ExtensionContext;
	for (const h of handlers.get("session_start") ?? []) h({ reason: "startup" }, ctx);
	const id = (t: string) => t;
	const editor = new CustomEditor(tui as never, { borderColor: id, selectList: { selectedPrefix: id, selectedText: id, description: id, scrollInfo: id, noMatch: id } as never }, { matches: () => false } as never, { embedWorkingStatus: true });
	editor.focused = false;
	editor.setText("fix the build");
	try {
		for (const width of WIDTHS) {
			const rows = editor.render(width).map(plain);
			assert.deepEqual(rows, [
				`╭${"─".repeat(width - 2)}╮`,
				`│ ${"fix the build".padEnd(width - 4)} │`,
				`╰${"─".repeat(width - 2)}╯`,
			], `width ${width}`);
		}
	} finally {
		for (const h of handlers.get("session_shutdown") ?? []) h({ reason: "quit" }, ctx);
	}
});

test("chat margins: a tool block has its rail in column 0 and two blank columns after its text", async () => {
	for (const width of WIDTHS) {
		const harness = await loadExtension({ style: "compact", fold: "toggle" }, { defaults: true });
		try {
			const id = `cm-${width}`;
			const outcome = CASES.bash!;
			announce(harness, id, "bash");
			const row = makeRow("bash", id, outcome.args, harness.definitions.get("bash"));
			row.setArgsComplete();
			harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: "bash", args: outcome.args });
			row.markExecutionStarted();
			advance(300);
			harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: false });
			row.updateResult({ ...text("ok"), isError: false } as never, false);
			row.setExpanded(true);
			const rows = row.render(width).map(plain).filter((r) => r.trim() !== "").map((r) => r.trimEnd());
			assert.ok(rows.every((r) => !r.startsWith(" ")), `no row starts with a margin column at ${width}: ${JSON.stringify(rows)}`);
			assert.ok(rows[0]!.startsWith("╭─\uE0B6bash\uE0B4 cd /project && npm test -- --"), `head at ${width}: ${JSON.stringify(rows)}`);
			if (width === 80) {
				assert.equal(rows[0], "╭─\uE0B6bash\uE0B4 cd /project && npm test -- --watch=false");
				assert.equal(rows[1], "│ ok");
				assert.match(rows[2]!, new RegExp(`^╰${COLLAPSE} collapse · `, "u"));
			}
			assert.ok(rows.some((r) => r === "│ ok" || r.startsWith("│ ")), `a rail body row at ${width}`);
			for (const r of rows) assert.ok(r.length <= width - 2, `two blank columns at the right (${width}): ${JSON.stringify(r)}`);
			if (width === 40) assert.ok(rows.some((r) => r.length === width - 2), `text fills up to column W-3 (${width}): ${JSON.stringify(rows)}`);
		} finally {
			shutdown(harness);
		}
	}
});

function thinkingMessage(blocks: unknown[]) {
	let transform: ((markdown: string, context: unknown) => string) | undefined;
	let input: ((data: string) => unknown) | undefined;
	const pi = { on: () => {}, registerMarkdownTransformer: (h: typeof transform) => { transform = h; } } as unknown as ExtensionAPI;
	const controller = new ThinkingCycleController(pi, () => true, () => NERD);
	let message: AssistantMessageComponent | undefined;
	controller.bind({ ui: { theme, setHiddenThinkingLabel: (l?: string) => { if (l !== undefined) message?.setHiddenThinkingLabel(l); }, onTerminalInput: (h: typeof input) => { input = h; return () => {}; } } } as unknown as ExtensionContext);
	message = new AssistantMessageComponent(
		{ content: blocks, stopReason: "stop", timestamp: Date.parse("2026-10-14T21:41:02.114Z") } as never,
		false, undefined, undefined, 1, [(m: string, c: unknown) => transform!(m, c)] as never,
	);
	return { message, press: () => input!("\x14"), end: () => controller.dispose() };
}

test("chat margins: a Thinking block has its rail in column 0", () => {
	for (const width of WIDTHS) {
		const block = thinkingMessage([{ type: "thinking", thinking: "Weighing the options.\n\nKeep the cache." }]);
		try {
			const folded = block.message.render(width).map(plain).filter((r) => r.trim() !== "").map((r) => r.trimEnd());
			assert.deepEqual(folded, ["╭─\uE0B6Thinking…\uE0B4", `╰${EXPAND} expand · 03:11`], `folded at ${width}`);
			block.press();
			const open = block.message.render(width).map(plain).filter((r) => r.trim() !== "").map((r) => r.trimEnd());
			assert.deepEqual(open, ["╭─\uE0B6Thinking…\uE0B4", "│ Weighing the options.", "│", "│ Keep the cache.", `╰${COLLAPSE} collapse · 03:11`], `expanded at ${width}`);
		} finally {
			block.end();
		}
	}
});

test("chat margins: the agent balloon has one blank row above it", () => {
	for (const width of WIDTHS) {
		const plainText = new AssistantMessageComponent({ role: "assistant", content: [{ type: "text", text: "agent reply here" }], stopReason: "stop", timestamp: Date.parse("2026-10-14T21:41:02.114Z") } as never);
		const rows = plainText.render(width).map(plain);
		assert.deepEqual(rows.map((r) => r.trimEnd()), ["", "╭──────────────────╮", "│ agent reply here │", "│╭─03:11───────────╯", "╰╯"], `at ${width}`);
		const block = thinkingMessage([{ type: "thinking", thinking: "Weighing." }, { type: "text", text: "agent reply here" }]);
		try {
			const all = block.message.render(width).map(plain).map((r) => r.trimEnd());
			const top = all.findIndex((r) => r.startsWith("╭──────"));
			assert.ok(top >= 1, JSON.stringify(all));
			assert.equal(all[top - 1], "", `a blank row above the balloon after Thinking at ${width}: ${JSON.stringify(all)}`);
			assert.ok(all[top - 2] !== "" , `exactly one blank row: ${JSON.stringify(all)}`);
		} finally {
			block.end();
		}
	}
});

test("chat margins: the user balloon ends in the last column and keeps three columns on its left", async () => {
	const { drawBalloon } = (await import("../lib/balloon.ts")) as { drawBalloon: (rows: string[], side: "user" | "agent", width: number) => string[] };
	for (const width of WIDTHS) {
		const rows = drawBalloon(["user prompt"], "user", width).map(plain);
		const pad = " ".repeat(width - 15);
		assert.deepEqual(rows, [
			`${pad}╭─────────────╮`,
			`${pad}│ user prompt │`,
			`${pad}╰────────────╮│`,
			`${pad}             ╰╯`,
		], `drawBalloon at ${width}`);
		const component = new UserMessageComponent("user prompt").render(width).map(plain);
		assert.deepEqual(component, rows, `UserMessageComponent at ${width}`);
		const long = drawBalloon(["x".repeat(200)], "user", width).map(plain);
		for (const r of long) {
			assert.equal(r.length, width, `last column at ${width}: ${JSON.stringify(r)}`);
			assert.ok(r.startsWith("   "), `far gap of three at ${width}: ${JSON.stringify(r)}`);
		}
		const wrapped = new UserMessageComponent("alpha beta gamma delta epsilon zeta eta theta iota kappa").render(width).map(plain);
		for (const r of wrapped.filter((x) => x.includes("│ "))) {
			const inner = r.slice(r.indexOf("│") + 2, r.lastIndexOf("│") - 1).trimEnd();
			assert.ok(inner.length <= width - 7, `wraps at pane-7 (${width}): ${JSON.stringify(inner)}`);
		}
	}
});

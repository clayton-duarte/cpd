/** Only the label text `Thinking…` is italic (SGR 3, contract 6.1.1, 6.9): the pills' caps, the loader frame, other words and the Thinking body are upright. */
import "./pinned-themes.ts";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import chalk from "chalk";
import { AssistantMessageComponent, CustomEditor, initTheme, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { WorkingStatusIndicator } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/status-indicator.js";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { ThinkingCycleController } from "../../compact-tools/extensions/compact-tools-thinking.ts";
import { ProgressController } from "../../compact-tools/extensions/compact-tools-progress.ts";
import { restoreClocks } from "./indicator-harness.ts";
import { MOON_FRAMES } from "./loader-frames.ts";

chalk.level = 3; // pi draws italic with chalk, which is colourless off a TTY: the bytes must be the ones a terminal gets
initTheme("dark", false);
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "thinking-label-italic-agent-"));
test.after(() => { restoreClocks(); rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }); });

const LEFT = "\u{E0B6}";
const RIGHT = "\u{E0B4}";
const NERD = { expand: "\u{F0B26}", collapse: "\u{F0B2C}" };
const ESC = "\x1b";
const LABEL = `${ESC}[3mThinking…${ESC}[23m`;
/** The Thinking title pill: grey 7, caps upright, the label italic inside the inverted text. */
const THINKING_PILL = `${ESC}[37m${LEFT}${ESC}[7m${LABEL}${ESC}[27m${ESC}[37m${RIGHT}${ESC}[39m`;
const plain = (line: string) => line.replace(/\x1b\][^\x07]*\x07/gu, "").replace(/\x1b\[[0-9;]*m/gu, "");
const strip = (value: string) => value.replace(/\x1b\[[0-9;]*m/gu, "");

function live(opts: { hide?: boolean; thinking?: string; markdownTheme?: boolean } = {}) {
	let transform: ((markdown: string, context: unknown) => string) | undefined;
	let input: ((data: string) => unknown) | undefined;
	const pi = { on: () => {}, registerMarkdownTransformer: (h: typeof transform) => { transform = h; } } as unknown as ExtensionAPI;
	const controller = new ThinkingCycleController(pi, () => true, () => NERD);
	const hide = opts.hide ?? false;
	const thinking = opts.thinking ?? "I will look at how the title is built.\n\nThen decide what the fold should hold.";
	const message = new AssistantMessageComponent({
		content: [{ type: "thinking", thinking }, { type: "text", text: "done" }], stopReason: "stop", timestamp: 0,
	} as never, hide, undefined, undefined, 1, [(m: string, c: unknown) => transform!(m, c)] as never);
	controller.bind({ ui: { theme, setHiddenThinkingLabel: (l?: string) => { if (l !== undefined) message.setHiddenThinkingLabel(l); }, onTerminalInput: (h: typeof input) => { input = h; return () => {}; } } } as unknown as ExtensionContext);
	let hidden = hide;
	return {
		draw: () => message.render(80),
		press: () => {
			const result = input!("\x14");
			if (result === undefined) { hidden = !hidden; message.setHideThinkingBlock(hidden); }
		},
		end: () => controller.dispose(),
	};
}

function titleRow(lines: string[]): string {
	const row = lines.find((line) => plain(line).includes(`${LEFT}Thinking…${RIGHT}`));
	assert.ok(row, `a Thinking title row in ${JSON.stringify(lines)}`);
	return row!;
}

test("the Thinking pill's text is italic and its caps are not, folded", () => {
	const block = live();
	try {
		const row = titleRow(block.draw());
		assert.ok(row.includes(THINKING_PILL), `the pill bytes, got ${JSON.stringify(row)}`);
	} finally { block.end(); }
});

test("the Thinking pill's text is italic and its caps are not, expanded", () => {
	const block = live();
	try {
		block.draw();
		block.press();
		const lines = block.draw();
		const row = titleRow(lines);
		assert.ok(row.includes(THINKING_PILL), `the pill bytes, got ${JSON.stringify(row)}`);
		const toggle = lines.find((line) => plain(line).includes("collapse"));
		assert.ok(toggle && !toggle.includes(`${ESC}[3m`), "the toggle row is upright");
	} finally { block.end(); }
});

test("the plain Thinking label is italic in the hidden view", () => {
	const block = live();
	try {
		block.draw();
		for (let i = 0; i < 3; i++) block.press(); // folded, expanded, folded, plain label
		const lines = block.draw().filter((line) => plain(line).trim() !== "");
		assert.deepEqual(lines.map((line) => plain(line).trim()), ["Thinking…", "done"], "the plain label");
		assert.ok(lines[0]!.includes(LABEL), `the label is italic, got ${JSON.stringify(lines[0])}`);
		assert.ok(!lines[1]!.includes(`${ESC}[3m`), "the reply is upright");
	} finally { block.end(); }
});

type Handler = (event: unknown) => void;
type Indicator = { frames: string[]; intervalMs?: number };

function progress(act: (handlers: Map<string, Handler>) => void) {
	const handlers = new Map<string, Handler>();
	const pi = { on: (name: string, handler: Handler) => handlers.set(name, handler), getThinkingLevel: () => "medium" } as unknown as ExtensionAPI;
	let message: string | undefined;
	let indicator: Indicator | undefined;
	const controller = new ProgressController(pi);
	controller.bind({
		isIdle: () => false,
		ui: { theme, setWorkingVisible: () => {}, setWorkingIndicator: (v?: Indicator) => { indicator = v; }, setWorkingMessage: (v?: string) => { message = v; } },
	} as unknown as ExtensionContext);
	act(handlers);
	return { message: message!, indicator, controller };
}

const WORKING_LABELS: Array<[string, (h: Map<string, Handler>) => void]> = [
	["Responding…", (h) => h.get("message_update")!({ assistantMessageEvent: { type: "text_delta" } })],
	["Preparing tools…", (h) => h.get("message_update")!({ assistantMessageEvent: { type: "toolcall_delta" } })],
	["Reading file…", (h) => h.get("tool_execution_start")!({ toolCallId: "1", toolName: "read", args: { path: "a.ts" } })],
	["Processing results…", (h) => { h.get("tool_execution_start")!({ toolCallId: "1", toolName: "bash", args: {} }); h.get("tool_execution_end")!({ toolCallId: "1" }); }],
];

test("the working pill's label is italic when it says Thinking… and its frame is not", () => {
	const { message, controller } = progress((h) => h.get("agent_start")!({}));
	try {
		assert.equal(message, `${ESC}[36m${LEFT}${ESC}[7m${LABEL}${ESC}[27m${ESC}[36m${RIGHT}${ESC}[39m`, "pi's working message, no frame");
	} finally { controller.dispose(); }
});

/** A real CustomEditor with the working status pi would build from what the extension sets. */
function editorWith(act: (handlers: Map<string, Handler>) => void) {
	const { message, indicator, controller } = progress(act);
	const ui = { requestRender: () => {}, terminal: { rows: 40, columns: 60 } };
	const editor = new CustomEditor(ui as never, { borderColor: (text: string) => text } as never, { matches: () => false } as never, { embedWorkingStatus: true } as never);
	const status = new WorkingStatusIndicator(ui as never, message, indicator as never, (text: string) => text);
	editor.setWorkingStatusIndicator(status);
	return { row: () => editor.render(60)[0]!, dispose: () => { status.dispose(); controller.dispose(); } };
}

/** The inner text of the pill of colour 6 in the top border, as drawn. */
const pillInner = (row: string) => {
	const open = `${ESC}[36m${LEFT}${ESC}[7m`;
	const close = `${ESC}[27m${ESC}[36m${RIGHT}`;
	const start = row.indexOf(open);
	assert.ok(start >= 0, `the pill opens in ${JSON.stringify(row)}`);
	const end = row.indexOf(close, start);
	assert.ok(end > start, `the pill closes in ${JSON.stringify(row)}`);
	return row.slice(start + open.length, end);
};

test("the working pill in the input border is italic on Thinking… only", () => {
	const w = editorWith((h) => h.get("agent_start")!({}));
	try {
		const inner = pillInner(w.row());
		assert.equal(strip(inner).slice(2), "Thinking…");
		assert.ok(MOON_FRAMES.includes([...strip(inner)][0]!), "the loader frame opens the pill");
		assert.ok(inner.endsWith(` ${LABEL}`), `the frame and the space are upright, the label italic, got ${JSON.stringify(inner)}`);
		assert.ok(!inner.slice(0, -LABEL.length).includes(`${ESC}[3m`), "nothing before the label is italic");
	} finally { w.dispose(); }
});

test("a working pill with any other label has no italic", () => {
	for (const [label, act] of WORKING_LABELS) {
		const { message, controller } = progress(act);
		const w = editorWith(act);
		try {
			assert.equal(strip(message).replace(LEFT, "").replace(RIGHT, ""), label);
			assert.ok(!message.includes(`${ESC}[3m`), `${label} message is upright`);
			assert.ok(!w.row().includes(`${ESC}[3m`), `${label} border is upright`);
		} finally { w.dispose(); controller.dispose(); }
	}
});

test("the Thinking body draws no italic: emphasis, inline code and highlighted code", () => {
	const original = theme.fg.bind(theme) as (token: string, text: string) => string;
	const italicTokens = new Set(["mdCode", "syntaxFunction", "syntaxType", "syntaxKeyword", "mdCodeBlock"]);
	// As pi's Theme.fg hook will draw them: code and highlighted code come back in SGR 3.
	(theme as any).fg = (token: string, text: string) => (italicTokens.has(token) ? `${ESC}[3m${original(token, text)}${ESC}[23m` : original(token, text));
	const block = live({ thinking: "An *em* and _em_ and ***both*** and `inline code` here.\n\n```ts\nfunction f(a: string): number { return 1; }\n```" });
	try {
		block.draw();
		block.press();
		const lines = block.draw();
		const body = lines.filter((line) => !plain(line).includes("Thinking…")).join("\n");
		assert.ok(plain(body).includes("inline code") && plain(body).includes("function"), "the body is drawn");
		assert.ok(!/\x1b\[3m/u.test(body), `no SGR 3 in the body: ${JSON.stringify(body)}`);
	} finally { block.end(); (theme as any).fg = original; delete (theme as any).fg; }
});

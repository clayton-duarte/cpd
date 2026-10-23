import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { AssistantMessageComponent, initTheme, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { ThinkingCycleController } from "../../compact-tools/extensions/compact-tools-thinking.ts";

import chalk from "chalk";
chalk.level = 3; // pi draws italic with chalk, which is colourless off a TTY: the bytes must be the ones a terminal gets
initTheme("dark", false);
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "thinking-plain-agent-"));
test.after(() => rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }));

const NERD = { expand: "\u{F0B26}", collapse: "\u{F0B2C}" };
const ITALIC = /\x1b\[3m/u;
const plain = (line: string) => line.replace(/\x1b\][^\x07]*\x07/gu, "").replace(/\x1b\[[0-9;]*m/gu, "").trim();

type Build = "after" | "before" | "streaming";

function live(thinking: string, opts: { hide?: boolean; build?: Build; reply?: string } = {}) {
	const build = opts.build ?? "after";
	let transform: ((markdown: string, context: unknown) => string) | undefined;
	let input: ((data: string) => unknown) | undefined;
	const pi = { on: () => {}, registerMarkdownTransformer: (h: typeof transform) => { transform = h; } } as unknown as ExtensionAPI;
	const controller = new ThinkingCycleController(pi, () => true, () => NERD);
	const reply = opts.reply ?? "done";
	const msg = (text: string) => ({ content: [{ type: "thinking", thinking: text }, { type: "text", text: reply }], stopReason: "stop" }) as never;
	const make = (text: string) => new AssistantMessageComponent(msg(text), opts.hide ?? false, undefined, undefined, 1, [(m: string, c: unknown) => transform!(m, c)] as never);
	let message: AssistantMessageComponent | undefined;
	if (build === "before") message = make(thinking);
	controller.bind({ ui: { theme, setHiddenThinkingLabel: (l?: string) => { if (l !== undefined) message?.setHiddenThinkingLabel(l); }, onTerminalInput: (h: typeof input) => { input = h; return () => {}; } } } as unknown as ExtensionContext);
	if (build === "after") message = make(thinking);
	if (build === "streaming") {
		message = make("");
		message.updateContent(msg(thinking.slice(0, 5)), true);
		message.updateContent(msg(thinking), true);
	}
	return {
		draw: () => message!.render(80),
		press: () => input!("\x14"),
		end: () => controller.dispose(),
	};
}

/** Expanded view: draw once so the controller learns the block has detail, then Ctrl+T. */
function expanded(thinking: string, build: Build = "after") {
	const block = live(thinking, { build });
	try {
		block.draw();
		block.press();
		return block.draw();
	} finally { block.end(); }
}

const noItalic = (lines: string[], what: string) =>
	assert.ok(!ITALIC.test(lines.join("\n")), `${what} draws SGR 3: ${JSON.stringify(lines.join("\n"))}`);

test("emphasis in thinking text draws no italic in the expanded view", () => {
	const lines = expanded("I think this is *really* hard.\n\nThe *user* said _other_ things.");
	noItalic(lines, "expanded view");
});

test("underscore, strong-and-emphasis and emphasis inside lists, quotes, headings, links and table cells draw no italic", () => {
	const samples: Record<string, string> = {
		underscore: "An _other_ idea.",
		both: "A ***both*** idea.",
		list: "- one\n- two *three*",
		quote: "> a *quoted* word",
		heading: "# A *heading* here",
		link: "See [a *link* label](https://example.com) now.",
		table: "| a | b |\n|---|---|\n| *x* | _y_ |",
	};
	for (const [name, text] of Object.entries(samples)) noItalic(expanded(text), name);
});

test("no italic in any view however the message was built or updated", () => {
	const text = "The *user* said _other_ things.\n\n- two *three*";
	for (const build of ["after", "before", "streaming"] as const) {
		const block = live(text, { build });
		try {
			const views: Record<string, string[]> = { folded: block.draw() };
			block.press();
			views.expanded = block.draw();
			for (const [view, lines] of Object.entries(views)) noItalic(lines, `${build} ${view}`);
		} finally { block.end(); }
		const hidden = live(text, { hide: true, build });
		try { noItalic(hidden.draw(), `${build} hidden`); } finally { hidden.end(); }
	}
});

test("bold and inline code in thinking text are unchanged", () => {
	const lines = expanded("Use **bold** and `code` here.");
	const out = lines.join("\n");
	assert.match(out, /\x1b\[1m[^\n]*bold/u, "bold keeps SGR 1");
	assert.match(out, /\x1b\[[0-9;]*m[^\n]*code/u, "inline code keeps its style");
	noItalic(lines, "bold and code");
});

test("the agent's reply keeps pi's emphasis", () => {
	const block = live("plain thought", { reply: "A *reply* with emphasis." });
	try {
		const lines = block.draw();
		const reply = lines.filter((l) => plain(l).includes("reply")).join("\n");
		assert.match(reply, ITALIC, `the reply draws SGR 3: ${JSON.stringify(reply)}`);
	} finally { block.end(); }
});

test("the words of an emphasis span stay in the thinking text", () => {
	const text = expanded("I think this is *really* hard, _other_ too, and - two *three*").map(plain).join("\n");
	for (const word of ["really", "other", "three"]) assert.ok(text.includes(word), `${word} stays: ${JSON.stringify(text)}`);
});

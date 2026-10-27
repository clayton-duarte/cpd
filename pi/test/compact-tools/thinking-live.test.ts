import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readdirSync, readFileSync } from "node:fs";
import test from "node:test";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { AssistantMessageComponent, initTheme, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { ThinkingCycleController } from "../../compact-tools/extensions/compact-tools-thinking.ts";

import chalk from "chalk";
chalk.level = 3; // pi draws italic with chalk, which is colourless off a TTY: the bytes must be the ones a terminal gets
initTheme("dark", false);
process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "thinking-live-agent-"));
test.after(() => rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }));

const LEFT = "\uE0B6";
const RIGHT = "\uE0B4";
const plain = (line: string) => line.replace(/\x1b\][^\x07]*\x07/gu, "").replace(/\x1b\[[0-9;]*m/gu, "").trim();
const rows = (lines: string[]) => lines.map(plain).filter((line) => line !== "");
const ITALIC = /\x1b\[3m/u;
const LABEL_ITALIC = /\x1b\[3mThinking…\x1b\[23m/gu;
const NERD = { expand: "\u{F0B26}", collapse: "\u{F0B2C}" };
const TEXT = "The user wants the pill to expand, so I will look at how the title is built here.\n\nThen decide what the fold should hold.";

function live(hideThinkingBlock = false) {
	let transform: ((markdown: string, context: unknown) => string) | undefined;
	let input: ((data: string) => unknown) | undefined;
	let label: string | undefined;
	const pi = { on: () => {}, registerMarkdownTransformer: (h: typeof transform) => { transform = h; } } as unknown as ExtensionAPI;
	const controller = new ThinkingCycleController(pi, () => true, () => NERD);
	const message = new AssistantMessageComponent({
		content: [{ type: "thinking", thinking: TEXT }, { type: "text", text: "done" }], stopReason: "stop", timestamp: TIME_MS,
	} as never, hideThinkingBlock, undefined, undefined, 1, [(m: string, c: unknown) => transform!(m, c)] as never);
	controller.bind({ ui: { theme, setHiddenThinkingLabel: (l?: string) => { label = l; if (l !== undefined) message.setHiddenThinkingLabel(l); }, onTerminalInput: (h: typeof input) => { input = h; return () => {}; } } } as unknown as ExtensionContext);
	const draw = () => message.render(80);
	return {
		draw,
		click: () => { const lines = draw(); return (message as any).handleMouse({ type: "click", button: "left", x: 3, y: 1, width: 80, height: lines.length }); },
		press: () => input!("\x14"),
		end: () => controller.dispose(),
		label: () => label,
	};
}

const EXPANDED = [
	`╭─${LEFT}Thinking…${RIGHT}`,
	"│ The user wants the pill to expand, so I will look at how the title is built",
	"│ here.",
	"│",
	"│ Then decide what the fold should hold.",
	"╰\u{F0B2C} collapse · 03:11",
	"done",
];
const FOLDED = [`╭─${LEFT}Thinking…${RIGHT}`, "╰\u{F0B26} expand · 03:11", "done"];

/**
 * String and template literals of a source holding `...`; a spread and a comment are not literals.
 * A small lexer (comments, quotes, template text, regex literals): the repo's TypeScript has no parser API to take.
 */
function literalsWithThreeDots(file: string): string[] {
	const code = readFileSync(file, "utf8");
	const found: string[] = [];
	const lineOf = (index: number) => code.slice(0, index).split("\n").length;
	const literal = (start: number, text: string) => {
		if (text.includes("...")) found.push(`${file}:${lineOf(start)}: ${JSON.stringify(text)}`);
	};
	const braces: number[] = []; // brace depth at which each open template substitution resumes the template
	let depth = 0;
	let last = ""; // the last significant character, to tell a regex literal from a division
	let i = 0;
	const template = () => { // i is just after the opening backtick or the closing brace of a substitution
		const start = i;
		let text = "";
		while (i < code.length) {
			const c = code[i];
			if (c === "\\") { text += code.slice(i, i + 2); i += 2; continue; }
			if (c === "`") { literal(start, text); i++; last = "`"; return; }
			if (c === "$" && code[i + 1] === "{") { literal(start, text); i += 2; braces.push(depth); depth++; last = "{"; return; }
			text += c; i++;
		}
	};
	while (i < code.length) {
		const c = code[i];
		if (c === "/" && code[i + 1] === "/") { while (i < code.length && code[i] !== "\n") i++; continue; }
		if (c === "/" && code[i + 1] === "*") { i = code.indexOf("*/", i + 2) + 2; continue; }
		if (c === "'" || c === '"') {
			const start = i++;
			let text = "";
			while (i < code.length && code[i] !== c) { if (code[i] === "\\") { text += code.slice(i, i + 2); i += 2; } else text += code[i++]; }
			i++; literal(start, text); last = c; continue;
		}
		if (c === "`") { i++; template(); continue; }
		if (c === "/" && (last === "" || "(,=:[!&|?{};".includes(last) || /\breturn\s*$/u.test(code.slice(Math.max(0, i - 8), i)))) {
			i++;
			let inClass = false;
			while (i < code.length && (code[i] !== "/" || inClass)) {
				if (code[i] === "\\") i++;
				else if (code[i] === "[") inClass = true;
				else if (code[i] === "]") inClass = false;
				i++;
			}
			i++; last = "/"; continue;
		}
		if (c === "{") depth++;
		if (c === "}") {
			depth--;
			if (braces.length > 0 && braces[braces.length - 1] === depth) { braces.pop(); i++; template(); continue; }
		}
		if (!/\s/u.test(c)) last = c;
		i++;
	}
	return found;
}

const piDir = join(import.meta.dirname, "..", "..");
const sourceFiles = (dir: string) => readdirSync(join(piDir, dir)).filter((name) => name.endsWith(".ts")).map((name) => join(piDir, dir, name));

test("no string literal drawn in pi's chat holds three dots", () => {
	const files = [...sourceFiles("compact-tools/extensions"), ...sourceFiles("lib"), ...sourceFiles(".")];
	assert.ok(files.length > 10, "the scan reads the sources");
	assert.deepEqual(files.flatMap(literalsWithThreeDots), [], "every ellipsis is the one character …");
});

test("the Thinking pill reads Thinking… whatever hideThinkingBlock says", () => {
	const folded = live();
	const hidden = live(true);
	try {
		assert.equal(rows(folded.draw())[0], `╭─${LEFT}Thinking…${RIGHT}`, "the pill");
		assert.ok(hidden.label() !== undefined, "the controller sets pi's hidden label");
		assert.equal(rows(hidden.draw())[0], `╭─${LEFT}Thinking…${RIGHT}`, "the pill with hideThinkingBlock true");
	} finally { folded.end(); hidden.end(); }
});

test("clicking the folded Thinking block shows its whole text on rails", () => {
	const block = live();
	try {
		block.draw();
		const click = block.click();
		assert.equal(click?.handled, true);
		assert.deepEqual(rows(block.draw()), EXPANDED, "the click must reach our fold, not pi's plain hidden label");
	} finally { block.end(); }
});

test("Ctrl+T on the folded Thinking block expands it to the whole text", () => {
	const block = live();
	try {
		block.draw(); // the controller learns the block has detail on its first draw, as in pi
		assert.deepEqual(block.press(), { consume: true });
		assert.deepEqual(rows(block.draw()), EXPANDED);
	} finally { block.end(); }
});

test("clicking an expanded Thinking block folds it again", () => {
	const block = live();
	try {
		block.draw();
		block.click();
		block.click();
		assert.deepEqual(rows(block.draw()), FOLDED, "folded again, not pi's plain hidden label");
	} finally { block.end(); }
});

test("no view of the Thinking block draws italic", () => {
	const block = live();
	const hidden = live(true);
	try {
		const views: Record<string, string[]> = { folded: block.draw() };
		block.press();
		views.expanded = block.draw();
		views.hidden = hidden.draw();
		for (const [name, lines] of Object.entries(views)) {
			assert.ok(!ITALIC.test(lines.join("\n").replace(LABEL_ITALIC, "")), `${name} view draws SGR 3: ${JSON.stringify(lines)}`);
		}
	} finally { block.end(); hidden.end(); }
});

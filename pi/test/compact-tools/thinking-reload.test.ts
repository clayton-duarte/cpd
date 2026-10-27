/** Reload after an install (docs/contract.md 5.3, 6.9): pi re-evaluates the extension's modules but keeps its own classes. */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { AssistantMessageComponent, initTheme, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";

import chalk from "chalk";
chalk.level = 3;
initTheme("dark", false);
process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "thinking-reload-agent-"));
test.after(() => rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }));

type Module = typeof import("../../compact-tools/extensions/compact-tools-thinking.ts");
const url = new URL("../../compact-tools/extensions/compact-tools-thinking.ts", import.meta.url).href;
const load = async (n: number) => (await import(`${url}?load=${n}`)) as Module;

const LEFT = "\uE0B6";
const RIGHT = "\uE0B4";
const plain = (line: string) => line.replace(/\x1b\][^\x07]*\x07/gu, "").replace(/\x1b\[[0-9;]*m/gu, "").trim();
const rows = (lines: string[]) => lines.map(plain).filter((line) => line !== "");
const ITALIC = /\x1b\[3m/u;
const LABEL_ITALIC = /\x1b\[3mThinking…\x1b\[23m/gu;
const NERD = { expand: "\u{F0B26}", collapse: "\u{F0B2C}" };
const TEXT = "The user wants the pill to expand, so I will look at how the title is built here.\n\nThen decide what the fold should hold.";
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

type Controller = { dispose(): void };
let transform: ((markdown: string, context: unknown) => string) | undefined;
/** One session start of a module load: new controller, bind. Returns it for the later dispose. */
async function start(mod: Module): Promise<Controller> {
	const pi = { on: () => {}, registerMarkdownTransformer: (h: typeof transform) => { transform = h; } } as unknown as ExtensionAPI;
	const controller = new mod.ThinkingCycleController(pi, () => true, () => NERD);
	controller.bind({ ui: { theme, setHiddenThinkingLabel: () => {}, onTerminalInput: () => () => {} } } as unknown as ExtensionContext);
	return controller;
}

/** Loads 1..count in order, each previous one disposed first, as pi's /reload does. */
async function reloaded(count: number, start0 = 1): Promise<Controller> {
	let previous: Controller | undefined;
	for (let n = start0; n < start0 + count; n++) {
		const mod = await load(n);
		previous?.dispose();
		previous = await start(mod);
	}
	return previous!;
}

function chat(thinking = TEXT, reply = "done") {
	const message = new AssistantMessageComponent({
		content: [{ type: "thinking", thinking }, { type: "text", text: reply }], stopReason: "stop", timestamp: TIME_MS,
	} as never, false, undefined, undefined, 1, [(m: string, c: unknown) => transform!(m, c)] as never);
	const draw = () => message.render(80);
	const click = () => { const lines = draw(); return (message as any).handleMouse({ type: "click", button: "left", x: 3, y: 1, width: 80, height: lines.length }); };
	return { draw, click };
}

test("a click expands the folded Thinking block after one reload", async () => {
	const controller = await reloaded(2);
	try {
		const block = chat();
		assert.deepEqual(rows(block.draw()), FOLDED);
		block.click();
		assert.deepEqual(rows(block.draw()), EXPANDED, "the click must reach the reloaded controller");
	} finally { controller.dispose(); }
});

test("a click folds the expanded Thinking block after two reloads", async () => {
	const controller = await reloaded(3, 11);
	try {
		const block = chat();
		block.draw();
		block.click();
		assert.deepEqual(rows(block.draw()), EXPANDED, "expanded after the first click");
		block.click();
		assert.deepEqual(rows(block.draw()), FOLDED, "folded after the second click");
	} finally { controller.dispose(); }
});

test("no view of the Thinking block draws italic after a reload", async () => {
	const controller = await reloaded(2, 21);
	try {
		const block = chat();
		const views: Record<string, string[]> = { folded: block.draw() };
		block.click();
		views.expanded = block.draw();
		const em = chat("An *em* and _em_ and ***both*** here.\n\nSecond paragraph.");
		views.emFolded = em.draw();
		em.click();
		views.emExpanded = em.draw();
		for (const [name, lines] of Object.entries(views)) {
			assert.ok(!ITALIC.test(lines.join("\n").replace(LABEL_ITALIC, "")), `${name} view draws SGR 3: ${JSON.stringify(lines)}`);
		}
		assert.ok(ITALIC.test(chat(TEXT, "a reply with *em* in it").draw().join("\n")), "the reply keeps pi's emphasis");
	} finally { controller.dispose(); }
});

test("a wrapper left by an older load is replaced at the next load", async () => {
	const prototype = AssistantMessageComponent.prototype as any;
	const original = prototype.updateContent;
	const stale = function (this: unknown, ...args: unknown[]) { return original.apply(this, args); };
	(stale as any).original = original;
	prototype.updateContent = stale;
	let controller = await reloaded(1, 31);
	try {
		let block = chat();
		block.draw();
		block.click();
		assert.deepEqual(rows(block.draw()), EXPANDED, "after the first load");
		controller.dispose();
		controller = await reloaded(1, 32);
		block = chat();
		block.draw();
		block.click();
		assert.deepEqual(rows(block.draw()), EXPANDED, "after a second dispose and load");
	} finally { controller.dispose(); }
});

/**
 * The compaction fold (docs/contract.md section 6.9, hs-d650): pi-core's compaction notice is drawn
 * as a cpd fold by pi/compact-tools/extensions/compact-tools-compaction.ts, in the compact style with
 * `fold: toggle` only. Head `╭─` (8) then the pill `[compaction]` (caps U+E0B6 / U+E0B4, block 7
 * inverted). Folded, one further row: `╰` (8), `toggle.expand` U+F0B26 and ` expand` (7), ` · ` (8)
 * and `Compacted from N tokens` (8), N = tokensBefore.toLocaleString(). Expanded: the same head, the
 * summary on `│ ` rails (rail 8, text 7), then `╰` (8), `toggle.collapse` U+F0B2C and ` collapse` (7),
 * ` · ` and the same metadata. `fold: status`, `style: off`, claude and codex keep pi's native row.
 * The component extends pi-tui's Box (paddingX = outputPad = 1, paddingY = 1), so rows are compared
 * after stripping ANSI, trimming and dropping blanks.
 */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import {
	CompactionSummaryMessageComponent,
	initTheme,
	type ExtensionAPI,
	type ExtensionContext,
} from "@earendil-works/pi-coding-agent";
import { install } from "@sinonjs/fake-timers";
import { CompactionFoldController } from "../../compact-tools/extensions/compact-tools-compaction.ts";
import { setClockFocused } from "../../compact-tools/extensions/compact-tools-clock.ts";
import { chrome, paint, RUNNING, thinkingPill } from "../../compact-tools/extensions/compact-tools-indices.ts";
import { LOADER_FRAMES, LOADER_INTERVAL_MS } from "../../compact-tools/extensions/compact-tools-loader.ts";

initTheme("dark", false);
// The user's own settings.json must not decide the view these tests draw: an empty agent directory of their own.
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "compaction-fold-agent-"));
test.after(() => rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }));

const plain = (line: string) => line.replace(/\x1b\][^\x07]*\x07/gu, "").replace(/\x1b\[[0-9;]*m/gu, "");
const LEFT = "\uE0B6";
const RIGHT = "\uE0B4";
const EXPAND = "\u{F0B26}";
const COLLAPSE = "\u{F0B2C}";
const NERD = { expand: EXPAND, collapse: COLLAPSE };
const ESC = "\x1b[";
const rows = (lines: string[]) => lines.map(plain).map((line) => line.trim()).filter((line) => line !== "");

const head = `╭─${LEFT}compaction${RIGHT}`;
const foldedRows = (tokensBefore = 150345) => [
	head,
	`╰${EXPAND} expand · Compacted from ${tokensBefore.toLocaleString()} tokens`,
];
const expandedRows = (summary = "Goal: look around", tokensBefore = 150345) => [
	head,
	`│ ${summary}`,
	`╰${COLLAPSE} collapse · Compacted from ${tokensBefore.toLocaleString()} tokens`,
];

/** A compaction notice bound to a fresh controller, as the extension binds it for a `fold: toggle` config. */
function notice(summary = "Goal: look around", tokensBefore = 150345) {
	const pi = { on: () => {} } as unknown as ExtensionAPI;
	const controller = new CompactionFoldController(pi, () => true, () => NERD);
	controller.bind({ ui: { theme } } as unknown as ExtensionContext);
	const component = new CompactionSummaryMessageComponent(
		{ role: "compactionSummary", tokensBefore, summary } as never,
		undefined,
		1,
	);
	const draw = () => component.render(80);
	return { component, controller, draw };
}

const click = (component: CompactionSummaryMessageComponent) =>
	component.handleMouse({
		type: "click", button: "left", x: 1, y: 1, width: 80, height: 3, screenX: 1, screenY: 1,
	} as never);

test("a compaction notice folds to the compaction pill and an expand toggle", () => {
	// Two token counts pin the `toLocaleString` formatting (150,345 and 7).
	for (const tokensBefore of [150345, 7]) {
		const block = notice("Goal: look around", tokensBefore);
		try {
			assert.deepEqual(rows(block.draw()), foldedRows(tokensBefore), String(tokensBefore));
		} finally {
			block.controller.dispose();
		}
	}
});

test("the compaction pill is inverted in 7 with caps 7", () => {
	const block = notice();
	try {
		const line = block.draw().find((row) => plain(row).includes(LEFT));
		assert.ok(line, "a head row with a pill");
		const left = line.indexOf(LEFT);
		const right = line.indexOf(RIGHT, left);
		assert.ok(right > left, "a right cap");
		assert.ok(line.slice(0, left).endsWith(`${ESC}37m`), `left cap in fg 7: ${JSON.stringify(line)}`);
		const body = line.slice(left + 1, right);
		assert.ok(body.startsWith(`${ESC}7m`) && body.endsWith(`${ESC}27m${ESC}37m`), `block inverted: ${JSON.stringify(body)}`);
		assert.ok(!body.includes(`${ESC}47m`) && !body.includes(`${ESC}30m`), `no bg 7, no text 0: ${JSON.stringify(body)}`);
		assert.ok(!/\x1b\[(36|46|96|106)m/u.test(line), "no cyan");
	} finally {
		block.controller.dispose();
	}
});

test("an expanded compaction notice shows the summary on rails and ends with collapse", () => {
	const block = notice();
	try {
		block.component.setExpanded(true);
		assert.deepEqual(rows(block.draw()), expandedRows());
		const body = block.draw().find((row) => plain(row).includes("Goal: look around"))!;
		assert.ok(body, "a body row with the summary");
		const rail = body.indexOf("│");
		assert.ok(body.slice(0, rail).endsWith(`${ESC}90m`), `rail 8 on ${JSON.stringify(body)}`);
		const at = body.indexOf("Goal");
		assert.ok(body.slice(rail, at).includes(`${ESC}37m`), `text 7 on ${JSON.stringify(body)}`);
	} finally {
		block.controller.dispose();
	}
	// A Markdown summary keeps pi's own rendering; only the rails and the words are pinned here.
	const markdown = notice("## Goal\nLook around");
	try {
		markdown.component.setExpanded(true);
		const body = rows(markdown.draw()).filter((row) => row.startsWith("│ "));
		assert.ok(body.some((row) => row.includes("Goal")), `a body row with Goal: ${JSON.stringify(body)}`);
		assert.ok(body.some((row) => row.includes("Look around")), `a body row with Look around: ${JSON.stringify(body)}`);
	} finally {
		markdown.controller.dispose();
	}
});

test("a click on the compaction row toggles the fold", () => {
	const block = notice();
	try {
		assert.deepEqual(rows(block.draw()), foldedRows());
		click(block.component);
		assert.deepEqual(rows(block.draw()), expandedRows(), "expanded after the first click");
		click(block.component);
		assert.deepEqual(rows(block.draw()), foldedRows(), "folded after the second click");
	} finally {
		block.controller.dispose();
	}
});

type Module = typeof import("../../compact-tools/extensions/compact-tools-compaction.ts");
const url = new URL("../../compact-tools/extensions/compact-tools-compaction.ts", import.meta.url).href;
const load = async (n: number) => (await import(`${url}?load=${n}`)) as Module;

type Controller = { dispose(): void };

/** One session start of a module load: new controller, bind. Returns it for the later dispose. */
async function start(mod: Module): Promise<Controller> {
	const pi = { on: () => {} } as unknown as ExtensionAPI;
	const controller = new mod.CompactionFoldController(pi, () => true, () => NERD);
	controller.bind({ ui: { theme } } as unknown as ExtensionContext);
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

test("the newest load owns the compaction row after a reload", async () => {
	const controller = await reloaded(2);
	try {
		const component = new CompactionSummaryMessageComponent(
			{ role: "compactionSummary", tokensBefore: 150345, summary: "Goal: look around" } as never,
			undefined,
			1,
		);
		assert.deepEqual(rows(component.render(80)), foldedRows(), "the newest load draws the fold");
	} finally {
		controller.dispose();
	}
});

test("fold: status and a style off keep pi's native compaction row", () => {
	// What the extension passes for `fold: status`, claude, codex or `style: off`: no fold.
	const pi = { on: () => {} } as unknown as ExtensionAPI;
	const controller = new CompactionFoldController(pi, () => true, () => undefined);
	controller.bind({ ui: { theme } } as unknown as ExtensionContext);
	try {
		const component = new CompactionSummaryMessageComponent(
			{ role: "compactionSummary", tokensBefore: 150345, summary: "Goal: look around" } as never,
			undefined,
			1,
		);
		const view = plain(component.render(80).join("\n"));
		assert.ok(view.includes("[compaction]"), "pi's own label");
		assert.ok(!view.includes(LEFT) && !view.includes(RIGHT) && !view.includes(EXPAND), "no cpd fold chrome");
	} finally {
		controller.dispose();
	}
});

// The running compaction fold (docs/contract.md section 6.9, RUNNING state; pro-d353): a two-row widget
// mounted from session_before_compact until session_compact, session_compact_failed or dispose.
const WIDGET = "compact-tools-compaction";
type Handler = (event: unknown, ctx: unknown) => unknown;

function running(options: { enabled?: boolean; toggle?: boolean; hasUI?: boolean } = {}) {
	setClockFocused(true);
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const handlers = new Map<string, Handler[]>();
	const pi = { on: (name: string, handler: Handler) => handlers.set(name, [...(handlers.get(name) ?? []), handler]) } as unknown as ExtensionAPI;
	const calls: { key: string; factory: unknown }[] = [];
	const context = {
		hasUI: options.hasUI ?? true,
		ui: { theme, setWidget: (key: string, factory: unknown) => calls.push({ key, factory }) },
	} as unknown as ExtensionContext;
	const controller = new CompactionFoldController(
		pi,
		() => options.enabled ?? true,
		() => (options.toggle === false ? undefined : NERD),
	);
	controller.bind(context);
	const emit = (name: string) => { for (const handler of handlers.get(name) ?? []) handler({ type: name }, context); };
	const renders = { count: 0 };
	const mounted = () => {
		const call = calls[calls.length - 1];
		assert.ok(call && typeof call.factory === "function", "a widget factory is mounted");
		const component = (call.factory as (tui: unknown, theme: unknown) => { render(width: number): string[] })(
			{ requestRender: () => renders.count++ },
			theme,
		);
		return component;
	};
	const cleanup = () => { controller.dispose(); clock.uninstall(); };
	return { calls, clock, controller, emit, mounted, renders, cleanup };
}

const runningRows = (frame: number) => [
	chrome("╭─") + thinkingPill("compaction"),
	chrome("╰") + paint(RUNNING, LOADER_FRAMES[frame % LOADER_FRAMES.length]!) + chrome(" · Compacting…"),
];

test("session_before_compact mounts the running compaction widget with the head and the running closing row", () => {
	const run = running();
	try {
		run.emit("session_before_compact");
		assert.equal(run.calls.length, 1, "one setWidget call");
		assert.equal(run.calls[0]!.key, WIDGET);
		assert.deepEqual(run.mounted().render(80), runningRows(0));
	} finally {
		run.cleanup();
	}
});

test("the running closing row steps one loader frame per loader interval", () => {
	const run = running();
	try {
		run.emit("session_before_compact");
		const component = run.mounted();
		for (let frame = 1; frame <= 3; frame++) {
			const before = run.renders.count;
			run.clock.tick(LOADER_INTERVAL_MS);
			assert.deepEqual(component.render(80), runningRows(frame), `frame ${frame}`);
			assert.equal(run.renders.count, before + 1, "a re-render is requested each tick");
		}
	} finally {
		run.cleanup();
	}
});

function clearsAndStops(event: "session_compact" | "session_compact_failed" | "dispose"): void {
	const run = running();
	try {
		run.emit("session_before_compact");
		assert.equal(run.clock.countTimers(), 1, "the tick is subscribed");
		if (event === "dispose") run.controller.dispose();
		else run.emit(event);
		assert.equal(run.calls.length, 2, "mount then clear");
		assert.deepEqual(run.calls[1], { key: WIDGET, factory: undefined });
		assert.equal(run.clock.countTimers(), 0, "no subscriber left");
		const after = run.renders.count;
		run.clock.tick(LOADER_INTERVAL_MS * 5);
		assert.equal(run.renders.count, after, "nothing changes after the clear");
		assert.equal(run.calls.length, 2);
	} finally {
		run.cleanup();
	}
}

test("session_compact clears the running compaction widget and stops its tick", () => clearsAndStops("session_compact"));

test("session_compact_failed clears the running compaction widget and stops its tick", () => clearsAndStops("session_compact_failed"));

test("dispose clears the running compaction widget and stops its tick", () => clearsAndStops("dispose"));

test("a second session_before_compact does not stack a second tick subscription", () => {
	const run = running();
	try {
		run.emit("session_before_compact");
		run.mounted();
		run.emit("session_before_compact");
		assert.equal(run.calls.length, 1, "nothing new mounted");
		assert.equal(run.clock.countTimers(), 1);
		const before = run.renders.count;
		run.clock.tick(LOADER_INTERVAL_MS);
		assert.equal(run.renders.count, before + 1, "one subscription, one request per tick");
		run.emit("session_compact");
		assert.equal(run.clock.countTimers(), 0, "one clear leaves no subscriber");
	} finally {
		run.cleanup();
	}
});

test("no running compaction widget is mounted when the fold is not toggle or the context has no UI", () => {
	for (const options of [{ enabled: false }, { toggle: false }, { hasUI: false }]) {
		const run = running(options);
		try {
			run.emit("session_before_compact");
			assert.equal(run.calls.length, 0, JSON.stringify(options));
			assert.equal(run.clock.countTimers(), 0, `${JSON.stringify(options)} subscribes to nothing`);
		} finally {
			run.cleanup();
		}
	}
});

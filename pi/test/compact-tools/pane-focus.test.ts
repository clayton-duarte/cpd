/**
 * A pi pane that is not in view is static (hs-d656, hs-d658, contract 6.9): an unreported pane is unfocused, the
 * shared clock runs only while the pane has focus, and the working indicator carries no frames for pi's Loader to
 * animate. The extension's own clock draws the loader frame in the working pill.
 */
import "./pinned-themes.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { install } from "@sinonjs/fake-timers";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { CustomEditor } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/custom-editor.js";
import { initTheme, theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { onTick } from "../../compact-tools/extensions/compact-tools-clock.ts";
import { editorFocused } from "../../compact-tools/extensions/compact-tools-editor.ts";

const home = mkdtempSync(join(tmpdir(), "pane-focus-"));
process.env.PI_CODING_AGENT_DIR ??= home;
test.after(() => rmSync(home, { recursive: true, force: true }));
initTheme("dark", false);

const FOCUS_IN = "\x1b[I";
const FOCUS_OUT = "\x1b[O";

type Handler = (event: unknown, ctx?: unknown) => void;
type InputHandler = (data: string) => { consume?: boolean; data?: string } | undefined;

interface Session {
	editor: CustomEditor;
	/** How many times the extension asked the TUI to render. */
	renders(): number;
	/** The options the extension last passed to ctx.ui.setWorkingIndicator. */
	indicator(): { frames: string[]; intervalMs?: number } | undefined;
	/** The terminal reports focus in (true) or out (false), through the path an extension sees. */
	focus(inside: boolean): void;
	/** Pi restarts the extension: session_shutdown, then session_start with reason reload. */
	reload(): void;
	/** Pi ends a session and starts the next with the given reason. */
	switchSession(reason: string): void;
	dispose(): void;
}

/** The real CustomEditor with compact-tools loaded, as cursor-blink.test.ts loads it. */
async function session(): Promise<Session> {
	const handlers = new Map<string, Handler[]>();
	const inputs: InputHandler[] = [];
	let renders = 0;
	let indicator: { frames: string[]; intervalMs?: number } | undefined;
	const compactTools = (await import("../../compact-tools/extensions/compact-tools.ts")).default;
	compactTools({
		on: (name: string, handler: Handler) => handlers.set(name, [...(handlers.get(name) ?? []), handler]),
		registerTool: () => {},
		registerMarkdownTransformer: () => {},
		registerCommand: () => {},
		registerShortcut: () => {},
		getThinkingLevel: () => "medium",
	} as unknown as ExtensionAPI);
	const tui = {
		terminal: { rows: 40, columns: 80 },
		requestRender: () => { renders++; },
	};
	const noop = () => () => {};
	const ui = new Proxy({} as Record<string, unknown>, {
		get: (_target, key) => {
			if (key === "theme") return theme;
			if (key === "onTerminalInput") return (handler: InputHandler) => { inputs.push(handler); return () => {}; };
			if (key === "setWorkingIndicator") return (value?: typeof indicator) => { if (value) indicator = value; };
			if (key === "setWidget") return (_key: string, factory: unknown) => { if (typeof factory === "function") factory(tui); };
			return noop;
		},
	});
	const ctx = { cwd: "/project", mode: "tui", isProjectTrusted: () => false, isIdle: () => true, ui } as unknown as ExtensionContext;
	const emit = (name: string, event: unknown) => { for (const handler of handlers.get(name) ?? []) handler(event, ctx); };
	emit("session_start", { reason: "startup" });
	const keybindings = { matches: () => false } as never;
	const editor = new CustomEditor(tui as never, { borderColor: (text: string) => text, selectList: { selectedPrefix: (t: string) => t, selectedText: (t: string) => t, description: (t: string) => t, scrollInfo: (t: string) => t, noMatch: (t: string) => t } as never }, keybindings, { embedWorkingStatus: true });
	editor.focused = false;
	return {
		editor,
		renders: () => renders,
		indicator: () => indicator,
		focus(inside) {
			for (const handler of inputs) handler(inside ? FOCUS_IN : FOCUS_OUT);
		},
		reload() {
			emit("session_shutdown", { reason: "reload" });
			emit("session_start", { reason: "reload" });
		},
		switchSession(reason) {
			emit("session_shutdown", { reason });
			emit("session_start", { reason });
		},
		dispose() {
			emit("session_shutdown", { reason: "quit" });
		},
	};
}

test("an unreported pane counts as unfocused", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session();
	try {
		assert.equal(editorFocused(), false, "no focus report has arrived, so the pane is unfocused");
	} finally { s.dispose(); clock.uninstall(); }
});

test("an unfocused pane schedules no clock and never ticks", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session();
	let ticks = 0;
	const stop = onTick(() => { ticks++; });
	try {
		assert.equal(clock.countTimers(), 0, "an unfocused pane owns no interval");
		clock.tick(10_000);
		assert.equal(ticks, 0, "nothing ticks while the pane is not in view");
	} finally { stop(); s.dispose(); clock.uninstall(); }
});

test("focus in starts the clock and focus out stops it and renders once", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session();
	let ticks = 0;
	const stop = onTick(() => { ticks++; });
	try {
		assert.equal(clock.countTimers(), 0, "the unreported pane starts static");
		s.focus(true);
		assert.equal(clock.countTimers(), 1, "focus in starts the shared clock");
		clock.tick(80);
		assert.equal(ticks, 1, "the focused pane ticks");
		const before = s.renders();
		s.focus(false);
		assert.equal(clock.countTimers(), 0, "focus out stops the clock");
		assert.equal(s.renders() - before, 1, "focus out asks for exactly one render of the static frame");
		clock.tick(10_000);
		assert.equal(ticks, 1, "no callback runs after focus out");
	} finally { stop(); s.dispose(); clock.uninstall(); }
});

test("a reload keeps the focus the pane reported", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session();
	let ticks = 0;
	const stop = onTick(() => { ticks++; });
	try {
		s.focus(true);
		assert.equal(editorFocused(), true, "the pane reported its field focused");
		assert.equal(clock.countTimers(), 1, "the focused pane runs the shared clock");
		s.reload();
		assert.equal(editorFocused(), true, "a reload keeps the focus the pane reported");
		assert.equal(clock.countTimers(), 1, "the reloaded focused pane runs the shared clock again");
		clock.tick(80);
		assert.equal(ticks, 1, "the reloaded focused pane still ticks");
	} finally { stop(); s.dispose(); clock.uninstall(); }
});

test("a fresh pane with no report is still unfocused", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session();
	try {
		assert.equal(editorFocused(), false, "a fresh pane has no focus report, so it is unfocused");
		assert.equal(clock.countTimers(), 0, "a fresh unfocused pane owns no clock");
	} finally { s.dispose(); clock.uninstall(); }
});

test("the working indicator carries no frames for pi to animate", async () => {
	const s = await session();
	try {
		const indicator = s.indicator();
		assert.ok(indicator, "the extension sets pi's working indicator");
		assert.deepEqual(indicator!.frames, [], "an empty frame list leaves pi's Loader without an interval");
	} finally { s.dispose(); }
});

test("new, resume and fork sessions keep the focus the pane reported and startup does not", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session();
	try {
		for (const reason of ["new", "resume", "fork"]) {
			s.focus(true);
			s.switchSession(reason);
			assert.equal(editorFocused(), true, `${reason} keeps the focus`);
			assert.equal(clock.countTimers(), 1, `${reason} keeps the shared clock running`);
		}
		s.switchSession("startup");
		assert.equal(editorFocused(), false, "startup begins unfocused");
		assert.equal(clock.countTimers(), 0, "startup owns no clock");
	} finally { s.dispose(); clock.uninstall(); }
});

test("a keystroke focuses a pane that reported nothing and asks one render", async () => {
	const clock = install({ toFake: ["setInterval", "clearInterval"] });
	const s = await session();
	try {
		const before = s.renders();
		s.editor.handleInput("x");
		assert.equal(editorFocused(), true, "the key is evidence of focus");
		assert.equal(clock.countTimers(), 1, "the shared clock runs");
		assert.equal(s.renders() - before, 1, "exactly one render asked");
	} finally { s.dispose(); clock.uninstall(); }
});

/**
 * Red tests for "The ask dialog never hangs" (docs/commands.md §15.13, Plan
 * ask-dialog-stuck, hs-n605): the inline questionnaire mount never hangs. A render
 * guard (RENDER_GUARD_TICK_MS ticks, displacement after RENDER_GUARD_LOST_TICKS
 * consecutive missing renders) resolves a displaced mount loud within ~3 s, and a
 * 2-hour bound (MOUNT_BOUND_MS) ends every mount that neither settles nor is
 * displaced. All fakes — no real terminal, no sleeps: fake timers drive the guard
 * ticks, and the fake `ctx.ui.custom` parks the component factory so the test
 * mounts the component itself, exactly as pi's TUI would.
 *
 * On origin/main (no guard, no bound) every named test is red for the right
 * reason: a displaced or never-settling mount resolves nothing, so the done-call
 * assertions fail. The unnamed regression pin at the end passes on main (there is
 * no interval at all) and must keep passing once the guard exists.
 */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { after, before } from "node:test";
import { install } from "@sinonjs/fake-timers";
import type { ExtensionAPI, ExtensionContext, Theme } from "@earendil-works/pi-coding-agent";
import type { TUI } from "@earendil-works/pi-tui";
import { registerAskUserQuestionTool } from "../../ask-user-question/ask-user-question.ts";
import { resolvePresentation } from "../../ask-user-question/presentation.ts";
import type { QuestionnaireSessionComponent } from "../../ask-user-question/state/questionnaire-session.ts";
import type { QuestionnaireResult } from "../../ask-user-question/tool/types.ts";

/** Pinned by the docs paragraph — the tests drive ticks of these exact durations. */
const RENDER_GUARD_TICK_MS = 1000;
const RENDER_GUARD_LOST_TICKS = 3;
const MOUNT_BOUND_MS = 2 * 60 * 60 * 1000;

/** The model-facing sentences of the docs paragraph, quoted verbatim (em dash included). */
const DISPLACED_SENTENCE =
	"Error: the question dialog disappeared before the user answered (another dialog took the single dialog slot). The user never saw the questions — do NOT treat this as a decline. Ask the questions as plain chat text instead, without using this tool.";

const STUCK_SENTENCE =
	"Error: the question dialog did not settle within 2 hours and was closed. The user never saw the questions — do NOT treat this as a decline. Ask the questions as plain chat text instead, without using this tool.";

// loadConfig reads $XDG_CONFIG_HOME/rpiv-ask-user-question/config.json at execute
// time and falls back to defaults when the file is absent: a fresh, config-less
// temp dir keeps the tests off the real user config (the collapse listener is only
// registered for overlay mounts with hideOnCollapse, which the fake onTerminalInput
// covers).
const configHome = mkdtempSync(join(tmpdir(), "ask-hang-config-"));
const savedXdgConfigHome = process.env.XDG_CONFIG_HOME;
before(() => {
	process.env.XDG_CONFIG_HOME = configHome;
});
after(() => {
	if (savedXdgConfigHome === undefined) delete process.env.XDG_CONFIG_HOME;
	else process.env.XDG_CONFIG_HOME = savedXdgConfigHome;
	rmSync(configHome, { recursive: true, force: true });
});

type ToolExecute = (
	toolCallId: string,
	params: unknown,
	signal: AbortSignal,
	onUpdate: () => void,
	ctx: ExtensionContext,
) => Promise<unknown>;

type CustomFactory = (
	tui: TUI,
	theme: Theme,
	keybindings: { matches(data: string, name: string): boolean },
	done: (result: QuestionnaireResult) => void,
) => QuestionnaireSessionComponent;

interface MountOptions {
	/** Claim the inline slot before executing, so the tool mounts as an overlay. */
	holdSlot?: boolean;
}

interface PendingMount {
	factory: CustomFactory;
	opts: { overlay: boolean };
	resolve: (value: unknown) => void;
}

interface MountHandle {
	clock: ReturnType<typeof install>;
	pending: PendingMount;
	component: QuestionnaireSessionComponent;
	/** The done the test hands the component factory; resolves the custom() promise. */
	done: (result: QuestionnaireResult) => void;
	calls: { count: number; result: QuestionnaireResult | undefined };
	invalidations: () => number;
	out: Promise<any>;
	/** The inline-slot release of the test's own claim (overlay test), if it claimed one. */
	releaseHeldSlot: (() => void) | undefined;
	tick: (ms: number) => void;
}

/** Minimal objects the session constructor accepts (it reads tui.terminal, requestRender, theme fg/bg/bold). */
const fakeTui = {
	terminal: { rows: 40, columns: 120 },
	hasOverlay: () => false,
	requestRender: () => {},
} as unknown as TUI;
const fakeTheme = {
	fg: (_color: string, text: string) => text,
	bg: (_color: string, text: string) => text,
	bold: (text: string) => text,
} as unknown as Theme;
const fakeKeybindings = { matches: () => false };

/** A valid question set that satisfies validateQuestionnaire (2 options, no reserved labels). */
const PARAMS = {
	questions: [
		{
			question: "Which one?",
			header: "Pick",
			options: [
				{ label: "a", description: "Pick the first option" },
				{ label: "b", description: "Pick the second option" },
			],
		},
	],
};

function trackComponent(component: QuestionnaireSessionComponent): {
	component: QuestionnaireSessionComponent;
	invalidations: () => number;
} {
	let invalidations = 0;
	// The render guard's own tick calls the mounted component's invalidate(); the tests
	// must tolerate those calls, so they are recorded (never asserted) here.
	return {
		invalidations: () => invalidations,
		component: {
			render: (width) => component.render(width),
			invalidate: () => {
				invalidations++;
				component.invalidate();
			},
			handleInput: (data) => component.handleInput(data),
		},
	};
}

/**
 * One `ask_user_question` execute through the real tool (registerAskUserQuestionTool
 * of ask-user-question.ts) to a fake custom mount. Fake timers are installed BEFORE
 * the execute; the fake `ctx.ui.custom` parks factory+opts+resolve; the test then calls
 * the factory itself with the fakes and holds the returned component, reading the
 * session's real module state (the inline-slot claim of resolvePresentation).
 */
function mount(options: MountOptions = {}): MountHandle {
	const clock = install({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
	const releaseHeldSlot = options.holdSlot ? resolvePresentation().release : undefined;

	let definition: { name: string; execute: ToolExecute } | undefined;
	const pi = {
		registerTool: (d: { name: string; execute: ToolExecute }) => {
			definition = d;
		},
		events: { emit: () => {} },
	} as unknown as ExtensionAPI;
	registerAskUserQuestionTool(pi);

	let pending: PendingMount | undefined;
	const ctx = {
		hasUI: true,
		mode: "tui",
		cwd: "",
		isProjectTrusted: () => true,
		ui: {
			custom: (factory: CustomFactory, opts: { overlay: boolean }) =>
				new Promise((resolve) => {
					pending = { factory, opts, resolve };
				}),
			notify: () => {},
			onTerminalInput: () => () => {},
		},
	} as unknown as ExtensionContext;

	const out = definition!.execute("id", PARAMS, new AbortController().signal, () => {}, ctx) as Promise<any>;

	assert.ok(pending, "execute must reach ctx.ui.custom synchronously");
	const calls: MountHandle["calls"] = { count: 0, result: undefined };
	const done = (result: QuestionnaireResult) => {
		calls.count++;
		calls.result = result;
		pending!.resolve(result);
	};
	const tracked = trackComponent(pending!.factory(fakeTui, fakeTheme, fakeKeybindings, done));
	return {
		clock,
		pending,
		component: tracked.component,
		done,
		calls,
		invalidations: tracked.invalidations,
		out,
		releaseHeldSlot,
		tick: (ms) => clock.tick(ms),
	};
}

/** The final tool result of a loud resolution, per the docs: error-shaped, coded, with the exact sentence. */
async function assertLoudResult(
	out: Promise<any>,
	error: "dialog_displaced" | "dialog_stuck",
	sentence: string,
): Promise<void> {
	const result = await out;
	assert.equal(result.isError, true);
	assert.deepEqual(result.details.answers, []);
	assert.equal(result.details.cancelled, true);
	assert.equal(result.details.error, error);
	assert.equal(result.content[0].text, sentence);
}

test("a displaced inline question resolves loud within three missing-render ticks", async () => {
	const h = mount();
	try {
		// The inline slot is free at file start (no prior mount claimed it): this is an
		// inline mount, and the fake records the opts.
		assert.equal(h.pending.opts.overlay, false);
		// Two ticks are not enough for three consecutive missing renders: still open.
		h.tick((RENDER_GUARD_LOST_TICKS - 1) * RENDER_GUARD_TICK_MS);
		assert.equal(h.calls.count, 0);
		// Four ticks total — safely past the third missing-render tick (~3 s).
		h.tick(2 * RENDER_GUARD_TICK_MS);
		assert.equal(h.calls.count, 1); // red on origin/main: nothing ever resolves
		assert.equal(h.calls.result?.error, "dialog_displaced");
		await assertLoudResult(h.out, "dialog_displaced", DISPLACED_SENTENCE);
	} finally {
		h.clock.uninstall();
	}
});

test("a rendered question stays open and resolves loud only once the renders stop", async () => {
	const h = mount();
	try {
		// Render every tick: the guard must not fire on a rendered dialog.
		for (let i = 0; i < 10; i++) {
			h.component.render(120);
			h.tick(RENDER_GUARD_TICK_MS);
		}
		assert.equal(h.calls.count, 0);
		// The renders stop: within four ticks the mount is displaced.
		for (let i = 0; i < 4; i++) h.tick(RENDER_GUARD_TICK_MS);
		assert.equal(h.calls.count, 1); // red on origin/main: nothing ever resolves
		assert.equal(h.calls.result?.error, "dialog_displaced");
		await assertLoudResult(h.out, "dialog_displaced", DISPLACED_SENTENCE);
	} finally {
		h.clock.uninstall();
	}
});

test("a mount that keeps rendering but never settles resolves loud on the two-hour bound", async () => {
	const h = mount();
	try {
		// Keep rendering every tick so the render guard never fires; never call done. The
		// 2-hour bound must end the mount instead (7200 ticks + one past the bound).
		for (let tick = 0; tick * RENDER_GUARD_TICK_MS <= MOUNT_BOUND_MS; tick++) {
			h.component.render(120);
			h.tick(RENDER_GUARD_TICK_MS);
		}
		assert.equal(h.calls.count, 1); // red on origin/main: never settles
		assert.equal(h.calls.result?.error, "dialog_stuck");
		await assertLoudResult(h.out, "dialog_stuck", STUCK_SENTENCE);
	} finally {
		h.clock.uninstall();
	}
});

test("an overlay mount is not subject to the render guard but is subject to the bound", async () => {
	const h = mount({ holdSlot: true });
	try {
		// With the inline slot held, the tool mounts as an overlay (the fake records the opts).
		assert.equal(h.pending.opts.overlay, true);
		// Overlay mounts stack and cannot be displaced: never rendering must not fire the
		// render guard, even after four ticks.
		h.tick(4 * RENDER_GUARD_TICK_MS);
		assert.equal(h.calls.count, 0);
		// The bound still applies to the overlay.
		h.tick(MOUNT_BOUND_MS + RENDER_GUARD_TICK_MS);
		assert.equal(h.calls.count, 1); // red on origin/main: the bound never fires
		assert.equal(h.calls.result?.error, "dialog_stuck");
		await assertLoudResult(h.out, "dialog_stuck", STUCK_SENTENCE);
	} finally {
		h.releaseHeldSlot?.();
		h.clock.uninstall();
	}
});

test("the guard's interval is cleared when the mount settles", async () => {
	const h = mount();
	try {
		// A normal done (Esc cancel shape, answers: [] and cancelled: true) settles the
		// mount. Passes on origin/main (there is no interval at all) and must keep passing
		// after the guard: no interval may outlive a settled mount.
		h.done({ answers: [], cancelled: true });
		const result = await h.out;
		assert.equal(result.details.cancelled, true);
		assert.equal(h.clock.countTimers(), 0);
	} finally {
		h.clock.uninstall();
	}
});

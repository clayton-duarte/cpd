/**
 * Red tests for "The ask dialog never hangs" (docs/commands.md §15.13, Plan
 * ask-dialog-slot): the render guard must tell a taken editor slot from a TUI that
 * is simply not drawing. The guard ticks every RENDER_GUARD_TICK_MS and asks
 * pi-tui whether its mounted component is still in the editor container
 * (`Tui.isComponentMounted`): while mounted it requests a frame
 * (`Tui.requestRender`) so the mounted question keeps being drawn and never
 * resolves loud; only when the component has left the container (the slot was
 * taken) does the mount resolve the loud `dialog_displaced` result. A 2-hour bound
 * (MOUNT_BOUND_MS) ends every mount that neither settles nor is displaced. All
 * fakes — no real terminal, no sleeps: fake timers drive the guard ticks, and the
 * fake `ctx.ui.custom` parks the component factory so the test mounts the
 * component itself, exactly as pi's TUI would.
 *
 * On origin/main (the old missing-render heuristic; no isComponentMounted or
 * requestRender path) the named tests are red for the right reason: a still-mounted
 * component that draws no frame is declared displaced after three missing renders
 * instead of staying open, a component that left the container is not noticed at
 * all until those three ticks, and no requestRender is ever recorded. The unnamed
 * regression pins at the end (the overlay scope and the timer leak) pass on
 * origin/main and must keep passing once the guard exists.
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
	/**
	 * Whether a `requestRender()` draws a frame. A live TUI draws one (the default);
	 * a test that models a dead frame source sets it false, so the component never
	 * renders even though the guard asks for a frame.
	 */
	renderOnRequest?: boolean;
}

/**
 * The fake pi-tui. `isComponentMounted` answers whether the mounted component is
 * still in the editor container (default true; a test flips `mounted` to false to
 * simulate the slot being taken). `requestRender` records every call; with
 * `renderOnRequest` it also draws the frame (renders the mounted component), as a
 * live pi-tui does — so a mounted question the guard keeps alive keeps rendering.
 */
interface FakeTui {
	terminal: { rows: number; columns: number };
	hasOverlay: () => boolean;
	/** The editor-container membership `isComponentMounted` reports. */
	mounted: boolean;
	requestRenderCalls: number;
	renderOnRequest: boolean;
	/** The mounted component's render, run when `requestRender` draws a frame. */
	renderFrame: (() => void) | undefined;
	requestRender: () => void;
	isComponentMounted: (component: QuestionnaireSessionComponent) => boolean;
}

function createFakeTui(renderOnRequest: boolean): FakeTui {
	const tui: FakeTui = {
		terminal: { rows: 40, columns: 120 },
		hasOverlay: () => false,
		mounted: true,
		requestRenderCalls: 0,
		renderOnRequest,
		renderFrame: undefined,
		requestRender: () => {
			tui.requestRenderCalls++;
			if (tui.renderOnRequest) tui.renderFrame?.();
		},
		isComponentMounted: () => tui.mounted,
	};
	return tui;
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
	/** The fake pi-tui: flip `mounted` to displace the component, read `requestRenderCalls`. */
	tui: FakeTui;
	/** The done the test hands the component factory; resolves the custom() promise. */
	done: (result: QuestionnaireResult) => void;
	calls: { count: number; result: QuestionnaireResult | undefined };
	invalidations: () => number;
	out: Promise<any>;
	/** The inline-slot release of the test's own claim (overlay test), if it claimed one. */
	releaseHeldSlot: (() => void) | undefined;
	tick: (ms: number) => void;
}

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
	// The guard's own tick may call the mounted component's invalidate(); the tests
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
	const tui = createFakeTui(options.renderOnRequest ?? true);

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
	const tracked = trackComponent(pending!.factory(tui as unknown as TUI, fakeTheme, fakeKeybindings, done));
	// The session construction itself requests a frame; count only the guard's ticks from here.
	tui.requestRenderCalls = 0;
	tui.renderFrame = () => {
		tracked.component.render(120);
	};
	return {
		clock,
		pending,
		component: tracked.component,
		tui,
		done,
		calls,
		invalidations: tracked.invalidations,
		out,
		releaseHeldSlot,
		tick: (ms) => clock.tick(ms),
	};
}

/**
 * Settle a mount that a test may have left open (a failing assertion throws before the
 * mount resolves). `execute()`'s teardown releases the module-level inline slot, so
 * without this a red test would leak the claim and force later mounts into the overlay
 * presentation, hiding the render guard. Harmless when the mount already settled.
 */
async function releaseMount(h: MountHandle): Promise<void> {
	h.done({ answers: [], cancelled: true });
	await h.out;
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

test("a still-mounted question draws no done call however many ticks pass without a render", async () => {
	// A dead frame source: the fake TUI records the guard's requestRender calls but
	// draws no frame — exactly the pi-tui state the old heuristic false-positived on.
	const h = mount({ renderOnRequest: false });
	try {
		// Far more ticks than RENDER_GUARD_LOST_TICKS: a component that is still in the
		// editor container must never be declared displaced.
		for (let i = 0; i < RENDER_GUARD_LOST_TICKS * 4; i++) h.tick(RENDER_GUARD_TICK_MS);
		assert.equal(h.calls.count, 0); // red on origin/main: done(dialog_displaced) at tick 3
		assert.equal(h.tui.mounted, true);
	} finally {
		await releaseMount(h);
		h.clock.uninstall();
	}
});

test("a question whose component left the editor container resolves loud with dialog_displaced", async () => {
	const h = mount({ renderOnRequest: false });
	try {
		assert.equal(h.pending.opts.overlay, false);
		// The slot was taken: pi-tui reports the component is no longer mounted, so the
		// next tick resolves loud at once (the old guard needed three missing renders).
		h.tui.mounted = false;
		h.tick(RENDER_GUARD_TICK_MS);
		assert.equal(h.calls.count, 1); // red on origin/main: tick 1 of 3, nothing resolves yet
		assert.equal(h.calls.result?.error, "dialog_displaced");
		await assertLoudResult(h.out, "dialog_displaced", DISPLACED_SENTENCE);
	} finally {
		await releaseMount(h);
		h.clock.uninstall();
	}
});

test("a mounted question is asked to redraw on each guard tick", async () => {
	const h = mount({ renderOnRequest: false });
	try {
		for (let i = 0; i < 5; i++) h.tick(RENDER_GUARD_TICK_MS);
		assert.equal(h.tui.requestRenderCalls, 5); // red on origin/main: 0, the guard only invalidates
		assert.equal(h.calls.count, 0);
	} finally {
		await releaseMount(h);
		h.clock.uninstall();
	}
});

test("a mount that keeps rendering but never settles resolves loud on the two-hour bound", async () => {
	const h = mount();
	try {
		// While mounted the guard requests a frame, so the question keeps rendering; it
		// never resolves itself. The 2-hour bound must end it (7200 ticks + one past). A
		// resolution before the bound is the old heuristic firing: stop and assert it.
		for (let tick = 0; tick * RENDER_GUARD_TICK_MS <= MOUNT_BOUND_MS; tick++) {
			h.tick(RENDER_GUARD_TICK_MS);
			if (h.calls.count > 0) break;
		}
		assert.equal(h.calls.count, 1); // red on origin/main: displaced at tick 3, before the bound
		assert.equal(h.calls.result?.error, "dialog_stuck");
		await assertLoudResult(h.out, "dialog_stuck", STUCK_SENTENCE);
	} finally {
		await releaseMount(h);
		h.clock.uninstall();
	}
});

test("an overlay mount is not subject to the render guard but is subject to the bound", async () => {
	const h = mount({ holdSlot: true });
	try {
		// With the inline slot held, the tool mounts as an overlay (the fake records the opts).
		assert.equal(h.pending.opts.overlay, true);
		// Overlay mounts stack and cannot be displaced: nothing may fire the render guard,
		// even after four ticks.
		h.tick(4 * RENDER_GUARD_TICK_MS);
		assert.equal(h.calls.count, 0);
		// The bound still applies to the overlay. Unnamed pin: passes on this branch
		// (the bound predates the guard change) and must keep passing.
		h.tick(MOUNT_BOUND_MS + RENDER_GUARD_TICK_MS);
		assert.equal(h.calls.count, 1);
		assert.equal(h.calls.result?.error, "dialog_stuck");
		await assertLoudResult(h.out, "dialog_stuck", STUCK_SENTENCE);
	} finally {
		h.releaseHeldSlot?.();
		await releaseMount(h);
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
		await releaseMount(h);
		h.clock.uninstall();
	}
});

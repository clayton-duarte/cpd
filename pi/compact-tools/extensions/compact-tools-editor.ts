/**
 * The input widget: pi-tui's Editor drawn as a rounded box in the Dango palette (docs/contract.md 6.1.1, 6.9
 * "The input widget"). Every row is the wall or corner, one padding space, the text, one padding space, the wall or
 * corner, with no margin column; the box spans the pane and the text starts at column 2 (a pane that is not compact style
 * with fold `toggle` keeps a margin column on each side and the text at column 3). The border is one colour by state: cyan 6
 * while the agent works, else bright blue 12 while the terminal pane has focus, else 15. The pane counts as focused from a focus-in
 * report, a key the editor takes or a left-button press in it; only a focus-out report unfocuses it (6.9). Pi's own `borderColor`
 * (thinking level, bash mode) is ignored. Indexed SGR only.
 */
import { Editor, truncateToWidth, visibleWidth, type TUI } from "@earendil-works/pi-tui";
import { CustomEditor, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { hookMethod } from "./compact-tools-hook.ts";
import { setClockFocused } from "./compact-tools-clock.ts";
import { italic } from "../../lib/italic.ts";
import { INDEX, RUNNING, THINKING_LABEL, paint, pill } from "./compact-tools-indices.ts";
import { LOADER_FRAMES } from "./compact-tools-loader.ts";

const STATE_KEY = Symbol.for("pi-compact-tools.editor.state");
const WIDGET_KEY = "compact-tools-editor";
/** Wall + padding on each side; a pane without the chat margins (not compact style with fold `toggle`) adds a margin column. */
const sideOf = (margins: boolean): number => margins ? 2 : 3;
/** Narrower than this the box has no room for text and pi's own rows are kept. */
const minWidth = (margins: boolean): number => sideOf(margins) * 2 + 2;
const FOCUS_IN = "\x1b[I";
const FOCUS_OUT = "\x1b[O";
/** A left-button press in SGR: button 0 (any modifier bits), motion bit 32 and wheel bit 64 clear, final byte M. */
const SGR_MOUSE = /^\x1b\[<(\d+);\d+;\d+M$/;
const isLeftPress = (data: string): boolean => {
	const match = SGR_MOUSE.exec(data);
	if (!match) return false;
	const button = Number(match[1]);
	return (button & 3) === 0 && (button & 32) === 0 && (button & 64) === 0;
};

/** Set by the bound controller: the one place focus changes. */
let focusSetter: ((focused: boolean) => void) | undefined;

interface EditorState {
	/** The terminal pane has focus: false at a pi startup, then set by input evidence or a focus report (6.9). */
	focused: boolean;
	/** How many times the pane gained focus; the cursor restarts its blink on a change. */
	gains: number;
	/** The words of the working label while the agent works, without padding. */
	words: string | undefined;
	/** The loader frame index, stepped by the progress label's tick of the shared clock. */
	frame: number;
	/** Whether the pane draws the chat margins of docs/contract.md 6.9; set by the bound controller. */
	margins: () => boolean;
}

function state(): EditorState {
	const holder = globalThis as typeof globalThis & { [STATE_KEY]?: EditorState };
	holder[STATE_KEY] ??= { focused: false, gains: 0, words: undefined, frame: 0, margins: () => true };
	return holder[STATE_KEY];
}

/** The pane focus state the box tracks; the cursor reads it so focus is not tracked twice. */
export function editorFocused(): boolean {
	return state().focused;
}

/** The count of focus gains so far. */
export function editorFocusGains(): number {
	return state().gains;
}

/** Progress tells the box what the working label says; undefined when nothing is working. */
export function setWorkingWords(words: string | undefined, frame = 0): void {
	state().words = words;
	state().frame = frame;
}

/** The fields of pi-tui's Editor this module reads and restores; some are private in its typings. */
type EditorLike = {
	workingStatusIndicator?: unknown;
	scrollOffset: number;
	renderedVisibleLineCount: number;
	renderedAutocompleteHeight: number;
	paddingX: number;
	borderColor: (text: string) => string;
};

// docs/contract.md section 6.9: a compaction indicator shows the Compacting pill even without working words.
const effectiveWords = (editor: EditorLike): string | undefined =>
	(editor.workingStatusIndicator as { kind?: string } | undefined)?.kind === "compaction" ? (state().words ?? "Compacting…") : state().words;

const borderIndex = (editor: EditorLike): number =>
	effectiveWords(editor) !== undefined && editor.workingStatusIndicator !== undefined ? RUNNING : state().focused ? INDEX.blue + 8 : INDEX.brightWhite;

const loaderFrame = (): string => LOADER_FRAMES[state().frame % LOADER_FRAMES.length]!;

function boxRows(editor: EditorLike, width: number, original: (width: number) => string[]): string[] {
	const margins = state().margins();
	const SIDE = sideOf(margins);
	const lead = margins ? "" : " ";
	if (width < minWidth(margins)) return original(width);
	const textWidth = width - SIDE * 2;
	const { paddingX, borderColor, workingStatusIndicator } = editor;
	const words = effectiveWords(editor);
	// Pi's rows are drawn plain at the text width; the box adds the border and the walls.
	editor.paddingX = 0;
	editor.borderColor = (text) => text;
	// Without the status the editor's own top border is the plain one (with the scroll label).
	editor.workingStatusIndicator = undefined;
	let rows: string[];
	try { rows = original(textWidth); }
	finally {
		editor.paddingX = paddingX;
		editor.borderColor = borderColor;
		editor.workingStatusIndicator = workingStatusIndicator;
	}
	const index = borderIndex(editor);
	const ink = (text: string) => paint(index, text);
	const bodyCount = editor.renderedVisibleLineCount;
	const inner = width - 2 - lead.length * 2;
	const out: string[] = [];
	const hidden = editor.scrollOffset;
	let top: string | undefined;
	if (words !== undefined && workingStatusIndicator !== undefined && textWidth >= 1) {
		const room = inner - 1 - 2 - 2;
		if (room >= 1) {
			const label = truncateToWidth(words, room, "…");
			const block = pill(RUNNING, `${loaderFrame()} ${words === THINKING_LABEL ? italic(label) : label}`);
			let rest = inner - 1 - 2 - 2 - visibleWidth(label);
			let tail = "";
			const scroll = hidden > 0 ? ` ↑ ${hidden} more ` : "";
			if (scroll && rest >= visibleWidth(scroll) + 2) {
				tail = `${"─".repeat(rest - visibleWidth(scroll) - 1)}${scroll}─`;
				rest = 0;
			}
			top = `${lead}${ink("╭─")}${block}${ink(`${tail || "─".repeat(rest)}╮`)}${lead}`;
		}
	}
	out.push(top ?? `${lead}${ink(`╭─${rows[0]}─╮`)}${lead}`);
	for (let i = 1; i <= bodyCount; i++) out.push(`${lead}${ink("│")} ${rows[i]} ${ink("│")}${lead}`);
	out.push(`${lead}${ink(`╰─${rows[bodyCount + 1]}─╯`)}${lead}`);
	for (const row of rows.slice(bodyCount + 2)) out.push(`${" ".repeat(SIDE)}${row}${" ".repeat(SIDE)}`);
	return out;
}

export function hookEditor(): void {
	// Any key the pane receives for its editor is evidence it has focus (6.9). CustomEditor handles extension shortcuts and
	// app actions before it calls Editor, so it is wrapped too; Editor's wrapper skips it so a key counts once.
	const focusOnKey = (self: unknown, args: unknown[], original: (...args: unknown[]) => unknown): unknown => {
		// A focus report that reaches the editor as a key is not evidence of focus.
		if (args[0] !== FOCUS_IN && args[0] !== FOCUS_OUT) focusSetter?.(true);
		return original.apply(self, args);
	};
	hookMethod(Editor.prototype, "handleInput", "editor.box.input", (self, args, original) =>
		self instanceof CustomEditor ? original.apply(self, args) : focusOnKey(self, args, original));
	hookMethod(CustomEditor.prototype, "handleInput", "custom-editor.box.input", focusOnKey);
	hookMethod(Editor.prototype, "render", "editor.box.render", (self, args, original) =>
		boxRows(self, args[0], (width) => original.call(self, width)));
	// A click's column is counted from the text column, in the text area's width.
	hookMethod(Editor.prototype, "handleMouse", "editor.box.mouse", (self: EditorLike, args, original) => {
		const event = args[0];
		const SIDE = sideOf(state().margins());
		if (event.width < SIDE * 2 + 2) return original.call(self, event);
		const { paddingX } = self;
		self.paddingX = 0;
		try { return original.call(self, { ...event, x: event.x - SIDE, width: event.width - SIDE * 2 }); }
		finally { self.paddingX = paddingX; }
	});
}

/** Tracks the terminal's focus report for the pane the editor lives in, and patches the Editor. */
export class EditorBoxController {
	private stopInput: (() => void) | undefined;

	/** `margins`: whether the pane draws the chat margins (compact style, fold `toggle`); read at every render. */
	private readonly margins: () => boolean;

	constructor(margins: () => boolean = () => true) {
		this.margins = margins;
	}

	bind(ctx: ExtensionContext, startup: boolean): void {
		this.release();
		state().margins = this.margins;
		hookEditor();
		// Only a pi startup has no report; reload, new, resume and fork keep the focus the pane reported (6.9).
		if (startup) state().focused = false;
		setClockFocused(state().focused);
		let tui: TUI | undefined;
		ctx.ui.setWidget(WIDGET_KEY, (host) => {
			tui = host;
			return { render: () => [], invalidate() {} };
		}, { placement: "belowEditor" });
		const set = (focused: boolean): void => {
			if (state().focused === focused) return;
			state().focused = focused;
			if (focused) state().gains++;
			setClockFocused(focused);
			tui?.requestRender();
		};
		focusSetter = set;
		const seen = (data: string): void => {
			if (data === FOCUS_IN) set(true);
			else if (data === FOCUS_OUT) set(false);
			else if (isLeftPress(data)) set(true);
		};
		const stopListener = ctx.ui.onTerminalInput((data) => {
			seen(data);
			return undefined;
		});
		// The fullscreen renderer consumes the reports in its own first listener, so no later listener sees them:
		// observe them where it handles them, then let it do so unchanged.
		const viewport = tui as unknown as { handleViewportInput?: (data: string) => unknown } | undefined;
		const own = viewport && typeof viewport.handleViewportInput === "function" ? viewport : undefined;
		if (own) {
			const original = own.handleViewportInput!;
			own.handleViewportInput = function (this: unknown, data: string) {
				seen(data);
				return original.call(this, data);
			};
		}
		this.stopInput = () => {
			stopListener();
			if (own) delete own.handleViewportInput;
			if (focusSetter === set) focusSetter = undefined;
		};
	}

	dispose(ctx?: ExtensionContext): void {
		this.release();
		setClockFocused(false);
		state().words = undefined;
		ctx?.ui.setWidget(WIDGET_KEY, undefined, { placement: "belowEditor" });
	}

	private release(): void {
		this.stopInput?.();
		this.stopInput = undefined;
	}
}

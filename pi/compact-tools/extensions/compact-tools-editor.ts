/**
 * The input widget: pi-tui's Editor drawn as a rounded box in the cockpit palette (docs/contract.md 6.1.1, 6.9
 * "The input widget"). Every row is a margin space, the wall or corner, one padding space, the text, one padding
 * space, the wall or corner, a margin space; the text starts at column 3. The border is one colour by state: cyan 6
 * while the agent works, else bright blue 12 while the terminal pane has focus, else 15. Pi's own `borderColor`
 * (thinking level, bash mode) is ignored. Indexed SGR only.
 */
import { Editor, truncateToWidth, visibleWidth, type TUI } from "@earendil-works/pi-tui";
import type { ExtensionContext } from "@earendil-works/pi-coding-agent";
import { hookMethod } from "./compact-tools-hook.ts";
import { INDEX, RUNNING, paint, pill } from "./compact-tools-indices.ts";
import { LOADER_FRAMES } from "./compact-tools-loader.ts";

const STATE_KEY = Symbol.for("pi-compact-tools.editor.state");
const WIDGET_KEY = "compact-tools-editor";
/** Margin + wall + padding on each side. */
const SIDE = 3;
/** Narrower than this the box has no room for text and pi's own rows are kept. */
const MIN_WIDTH = SIDE * 2 + 2;
const FOCUS_IN = "\x1b[I";
const FOCUS_OUT = "\x1b[O";

interface EditorState {
	/** The terminal pane has focus; true before the first report. */
	focused: boolean;
	/** The words of the working label while the agent works, without padding. */
	words: string | undefined;
	/** The loader frame index, stepped by the progress label's tick of the shared clock. */
	frame: number;
}

function state(): EditorState {
	const holder = globalThis as typeof globalThis & { [STATE_KEY]?: EditorState };
	holder[STATE_KEY] ??= { focused: true, words: undefined, frame: 0 };
	return holder[STATE_KEY];
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

const borderIndex = (editor: EditorLike): number =>
	state().words !== undefined && editor.workingStatusIndicator !== undefined ? RUNNING : state().focused ? INDEX.blue + 8 : INDEX.brightWhite;

const loaderFrame = (): string => LOADER_FRAMES[state().frame % LOADER_FRAMES.length]!;

function boxRows(editor: EditorLike, width: number, original: (width: number) => string[]): string[] {
	if (width < MIN_WIDTH) return original(width);
	const textWidth = width - SIDE * 2;
	const { paddingX, borderColor, workingStatusIndicator } = editor;
	// Pi's rows are drawn plain at the text width; the box adds the border, the walls and the margins.
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
	const inner = width - 4;
	const out: string[] = [];
	const words = state().words;
	const hidden = editor.scrollOffset;
	let top: string | undefined;
	if (words !== undefined && workingStatusIndicator !== undefined && textWidth >= 1) {
		const room = inner - 2 - 2 - 2;
		if (room >= 1) {
			const label = truncateToWidth(words, room, "…");
			const block = pill(RUNNING, `${loaderFrame()} ${label}`);
			let rest = inner - 2 - 2 - 2 - visibleWidth(label);
			let tail = "";
			const scroll = hidden > 0 ? ` ↑ ${hidden} more ` : "";
			if (scroll && rest >= visibleWidth(scroll) + 2) {
				tail = `${"─".repeat(rest - visibleWidth(scroll) - 1)}${scroll}─`;
				rest = 0;
			}
			top = ` ${ink("╭──")}${block}${ink(`${tail || "─".repeat(rest)}╮`)} `;
		}
	}
	out.push(top ?? ` ${ink(`╭─${rows[0]}─╮`)} `);
	for (let i = 1; i <= bodyCount; i++) out.push(` ${ink("│")} ${rows[i]} ${ink("│")} `);
	out.push(` ${ink(`╰─${rows[bodyCount + 1]}─╯`)} `);
	for (const row of rows.slice(bodyCount + 2)) out.push(`${" ".repeat(SIDE)}${row}${" ".repeat(SIDE)}`);
	return out;
}

export function hookEditor(): void {
	hookMethod(Editor.prototype, "render", "editor.box.render", (self, args, original) =>
		boxRows(self, args[0], (width) => original.call(self, width)));
	// A click's column is counted from the text column, in the text area's width.
	hookMethod(Editor.prototype, "handleMouse", "editor.box.mouse", (self: EditorLike, args, original) => {
		const event = args[0];
		if (event.width < MIN_WIDTH) return original.call(self, event);
		const { paddingX } = self;
		self.paddingX = 0;
		try { return original.call(self, { ...event, x: event.x - SIDE, width: event.width - SIDE * 2 }); }
		finally { self.paddingX = paddingX; }
	});
}

/** Tracks the terminal's focus report for the pane the editor lives in, and patches the Editor. */
export class EditorBoxController {
	private stopInput: (() => void) | undefined;

	bind(ctx: ExtensionContext): void {
		this.release();
		hookEditor();
		state().focused = true;
		let tui: TUI | undefined;
		ctx.ui.setWidget(WIDGET_KEY, (host) => {
			tui = host;
			return { render: () => [], invalidate() {} };
		}, { placement: "belowEditor" });
		this.stopInput = ctx.ui.onTerminalInput((data) => {
			if (data !== FOCUS_IN && data !== FOCUS_OUT) return undefined;
			state().focused = data === FOCUS_IN;
			tui?.requestRender();
			return undefined;
		});
	}

	dispose(ctx?: ExtensionContext): void {
		this.release();
		state().words = undefined;
		ctx?.ui.setWidget(WIDGET_KEY, undefined, { placement: "belowEditor" });
	}

	private release(): void {
		this.stopInput?.();
		this.stopInput = undefined;
	}
}

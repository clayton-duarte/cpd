/**
 * The input cursor: one Dango-index cell that blinks and cycles only while the terminal pane has focus (hs-d649).
 * Pi's Editor wraps the cursor cell in fake-cursor markers; on a visible phase this paints the cell with the
 * cycle's index and resets the foreground after it, so pi keeps the reverse video and the hardware cursor.
 * On the off phase, and while unfocused, the fake-cursor markers are removed and the character stays plain.
 * When pi's hardware cursor is shown the extension draws and animates nothing.
 *
 * A keystroke pauses the 500 ms blink and the cursor holds the colour it had (hs-d681); the blink resumes 500 ms
 * after the last keystroke, on a visible phase in that same colour. The cursor blinks as before while idle.
 */
import { Editor } from "@earendil-works/pi-tui";
import { CustomEditor } from "@earendil-works/pi-coding-agent";
import { onTick } from "./compact-tools-clock.ts";
import { editorFocused, editorFocusGains } from "./compact-tools-editor.ts";
import { hookMethod } from "./compact-tools-hook.ts";
import { fgOpen } from "./compact-tools-indices.ts";

/** Pi-tui's zero-width hardware-cursor marker, and the fake-cursor span pi's TUI later reverses. */
const CURSOR_MARKER = "\x1b_pi:c\x07";
const START = "\x1b_pi:fc\x07";
const END = "\x1b_pi:/fc\x07";
const DEFAULT = "\x1b[39m";
/** One phase per 500 ms: visible then off, each 500 ms. */
const PERIOD_MS = 500;
/** The base indices of the cycle: magenta 5, white 7, green 2. */
const CYCLE = [5, 7, 2] as const;

/** The part of pi-tui's TUI this module reads. */
type TuiLike = {
	requestRender?: () => void;
	getShowHardwareCursor?: () => boolean;
};

const stripMarkers = (rows: string[]): string[] =>
	rows.map((row) => (row.includes(START) || row.includes(END)) ? row.replaceAll(START, "").replaceAll(END, "") : row);

const paintMarkers = (rows: string[], index: number): string[] =>
	rows.map((row) => {
		if (!row.includes(START) && !row.includes(END)) return row;
		// Pi emits its hardware marker only while the editor itself has focus; add it here when the pane is
		// focused but the editor is not, so the TUI keeps positioning the hardware cursor at the cell.
		const marked = row.includes(CURSOR_MARKER) ? row : row.replace(START, `${CURSOR_MARKER}${START}`);
		return marked.replace(START, `${fgOpen(index)}${START}`).replace(END, `${END}${DEFAULT}`);
	});

export class CursorController {
	private stopTick: (() => void) | undefined;
	private phase = 0;
	/** The focus gains the last render has seen. */
	private gains = 0;
	private bound = false;
	/** The wall clock of the last keystroke; the cursor stays solid until PERIOD_MS of idle have passed. */
	private lastKeystrokeAt = Number.NEGATIVE_INFINITY;
	private tui: TuiLike | undefined;

	bind(): void {
		this.release();
		this.phase = 0;
		this.bound = true;
		this.subscribe();
		hookMethod(Editor.prototype, "render", "editor.cursor", (self, args, original) =>
			this.render(self, args, original));
		// CustomEditor takes some keys before Editor does; Editor's wrapper skips it so a key holds the cursor once.
		hookMethod(Editor.prototype, "handleInput", "editor.cursor.input", (self, args, original) =>
			self instanceof CustomEditor ? original.apply(self, args) : this.input(self, args, original));
		hookMethod(CustomEditor.prototype, "handleInput", "custom-editor.cursor.input", (self, args, original) =>
			this.input(self, args, original));
	}

	dispose(): void {
		this.bound = false;
		this.release();
		this.phase = 0;
	}

	private release(): void {
		this.stopTick?.();
		this.stopTick = undefined;
		this.lastKeystrokeAt = Number.NEGATIVE_INFINITY;
	}

	/** One phase per tick, the render reads it; the shared clock holds no timer while the pane is unfocused. */
	private subscribe(): void {
		this.stopTick ??= onTick(() => {
			// The cursor stays solid while a keystroke is held; the blink counts only idle time.
			if (Date.now() - this.lastKeystrokeAt < PERIOD_MS) return;
			this.phase += 1;
			this.tui?.requestRender?.();
		}, PERIOD_MS);
	}

	private input(self: unknown, args: unknown[], original: (...args: unknown[]) => unknown): unknown {
		const result = original.apply(self, args);
		if (!this.bound) return result;
		this.lastKeystrokeAt = Date.now();
		this.subscribe();
		this.tui?.requestRender?.();
		return result;
	}

	private render(self: unknown, args: unknown[], original: (...args: unknown[]) => unknown): string[] {
		const rows = original.apply(self, args) as string[];
		if (!this.bound) return rows;
		const tui = (self as { tui?: TuiLike }).tui;
		this.tui = tui;
		const hardware = tui?.getShowHardwareCursor?.() ?? false;
		if (hardware) {
			this.release();
			return rows;
		}
		if (!editorFocused()) {
			this.release();
			return stripMarkers(rows);
		}
		// A focus gain draws the cursor on its visible phase, in the colour it had (6.9).
		const gains = editorFocusGains();
		if (gains !== this.gains) {
			this.gains = gains;
			this.phase -= this.phase % 2;
		}
		this.subscribe();
		const index = CYCLE[Math.floor(this.phase / 2) % CYCLE.length]!;
		// While a keystroke is held the cursor stays solid in the current cycle colour.
		if (Date.now() - this.lastKeystrokeAt < PERIOD_MS) return paintMarkers(rows, index);
		if (this.phase % 2 !== 0) return stripMarkers(rows);
		return paintMarkers(rows, index);
	}
}

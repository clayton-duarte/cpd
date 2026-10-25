/**
 * The input cursor: one Dango-index cell that blinks and cycles only while the terminal pane has focus (hs-d649).
 * Pi's Editor wraps the cursor cell in fake-cursor markers; on a visible phase this paints the cell with the
 * cycle's index and resets the foreground after it, so pi keeps the reverse video and the hardware cursor.
 * On the off phase, and while unfocused, the fake-cursor markers are removed and the character stays plain.
 * When pi's hardware cursor is shown the extension draws and animates nothing.
 */
import { Editor } from "@earendil-works/pi-tui";
import { onTick } from "./compact-tools-clock.ts";
import { editorFocused } from "./compact-tools-editor.ts";
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
	private bound = false;

	bind(): void {
		this.release();
		this.phase = 0;
		this.bound = true;
		hookMethod(Editor.prototype, "render", "editor.cursor", (self, args, original) =>
			this.render(self, args, original));
	}

	dispose(): void {
		this.bound = false;
		this.release();
		this.phase = 0;
	}

	private release(): void {
		this.stopTick?.();
		this.stopTick = undefined;
	}

	private render(self: unknown, args: unknown[], original: (...args: unknown[]) => unknown): string[] {
		const rows = original.apply(self, args) as string[];
		if (!this.bound) return rows;
		const tui = (self as { tui?: TuiLike }).tui;
		const hardware = tui?.getShowHardwareCursor?.() ?? false;
		if (hardware) {
			this.release();
			return rows;
		}
		if (!editorFocused()) {
			this.release();
			return stripMarkers(rows);
		}
		if (!this.stopTick) {
			// Start lazily on the first render while focused: one phase per tick, the render reads it.
			this.stopTick = onTick(() => {
				this.phase += 1;
				tui?.requestRender?.();
			}, PERIOD_MS);
		}
		if (this.phase % 2 !== 0) return stripMarkers(rows);
		const index = CYCLE[Math.floor(this.phase / 2) % CYCLE.length]!;
		return paintMarkers(rows, index);
	}
}

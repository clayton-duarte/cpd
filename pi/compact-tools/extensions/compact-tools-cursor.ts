/**
 * The input cursor: one Dango-index cell that blinks and cycles only while the terminal pane has focus (hs-d649).
 * Pi's Editor wraps the cursor cell in fake-cursor markers; on a visible phase this paints the cell with the
 * cycle's index and resets the foreground after it, so pi keeps the reverse video and the hardware cursor.
 * On the off phase, and while unfocused, the fake-cursor markers are removed and the character stays plain.
 * When pi's hardware cursor is shown the extension draws and animates nothing.
 *
 * A keystroke pauses the 500 ms blink and the cursor holds the colour it had (hs-d676); each inserted character
 * extends an inverse-video highlight trail over the characters just typed in that held colour, one per deleted
 * character shrinks it, and a newline clears it. After 250 ms idle the trail collapses in one step; the blink
 * then resumes on a visible phase, holding the same colour.
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
/** The SGR pair that opens and closes the inverse-video keystroke trail (hs-d676). */
const REVERSE = "\x1b[7m";
const UNREVERSE = "\x1b[27m";
/** One phase per 500 ms: visible then off, each 500 ms. */
const PERIOD_MS = 500;
/** The idle collapse check tick (hs-d676); a held trail is watched at this cadence. */
const COLLAPSE_MS = 50;
/** Keystroke idle after which a held trail collapses in one step (hs-d676). */
const IDLE_MS = 250;
/** The base indices of the cycle: magenta 5, white 7, green 2. */
const CYCLE = [5, 7, 2] as const;

/** The part of pi-tui's TUI this module reads. */
type TuiLike = {
	requestRender?: () => void;
	getShowHardwareCursor?: () => boolean;
};

/** Any escape pi writes into a rendered row: an indexed/truecolour SGR or a zero-width pi marker. */
const ESCAPE = /^(?:\x1b\[[0-9;]*m|\x1b_pi:[^\x07]*\x07)/;

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

/** The index just past each visible (non-escape) character in `text`, in order. */
function visibleOffsets(text: string): number[] {
	const offsets: number[] = [];
	for (let at = 0; at < text.length;) {
		const match = ESCAPE.exec(text.slice(at));
		if (match) { at += match[0].length; continue; }
		offsets.push(at);
		at += 1;
	}
	return offsets;
}

/**
 * Wrap the last `trail` rendered characters before the cursor cell in reverse video, in the held colour.
 * The open escape sits before the first highlighted character (which may be on an earlier, wrapped row) and
 * the close sits immediately before the cursor's own colour, so the cell keeps the held colour after it.
 */
function highlight(rows: string[], index: number, trail: number): string[] {
	const escape = `${fgOpen(index)}${START}`;
	const guarded = `${CURSOR_MARKER}${escape}`;
	let cursorRow = -1;
	let cursorAt = -1;
	for (let row = 0; row < rows.length; row += 1) {
		const at = rows[row]!.indexOf(guarded);
		if (at >= 0) { cursorRow = row; cursorAt = at; break; }
		const bare = rows[row]!.indexOf(escape);
		if (bare >= 0) { cursorRow = row; cursorAt = bare; break; }
	}
	if (cursorRow < 0) return rows;
	const before: Array<{ row: number; at: number }> = [];
	for (let row = 0; row < cursorRow; row += 1) {
		for (const at of visibleOffsets(rows[row]!)) before.push({ row, at });
	}
	for (const at of visibleOffsets(rows[cursorRow]!.slice(0, cursorAt))) before.push({ row: cursorRow, at });
	const from = Math.max(0, before.length - trail);
	const start = before[from];
	if (!start) return rows;
	const out = rows.slice();
	// Close on the cursor row first (a higher index), then open, so the open index stays valid when both are
	// on the same row.
	out[cursorRow] = out[cursorRow]!.slice(0, cursorAt) + UNREVERSE + DEFAULT + out[cursorRow]!.slice(cursorAt);
	out[start.row] = out[start.row]!.slice(0, start.at) + `${fgOpen(index)}${REVERSE}` + out[start.row]!.slice(start.at);
	return out;
}

export class CursorController {
	private stopTick: (() => void) | undefined;
	private stopCollapse: (() => void) | undefined;
	private phase = 0;
	private bound = false;
	/** The inverse-video trail length: 0 means the plain blink, positive means the trail is held. */
	private trail = 0;
	/** The colour the cursor holds while a trail is non-empty; undefined when no trail. */
	private heldColour: number | undefined;
	/** The wall clock of the last keystroke, used to start the idle collapse after IDLE_MS. */
	private lastKeystrokeAt = 0;
	private tui: TuiLike | undefined;

	bind(): void {
		this.release();
		this.phase = 0;
		this.bound = true;
		hookMethod(Editor.prototype, "render", "editor.cursor", (self, args, original) =>
			this.render(self, args, original));
		hookMethod(Editor.prototype, "handleInput", "editor.cursor.input", (self, args, original) =>
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
		this.stopCollapse?.();
		this.stopCollapse = undefined;
		this.trail = 0;
		this.heldColour = undefined;
		this.lastKeystrokeAt = 0;
	}

	private input(self: unknown, args: unknown[], original: (...args: unknown[]) => unknown): unknown {
		const before = (self as { getText: () => string }).getText();
		const result = original.apply(self, args);
		if (!this.bound || !editorFocused()) return result;
		const after = (self as { getText: () => string }).getText();
		if (after.split("\n").length > before.split("\n").length) {
			// A newline was inserted: no highlight, exactly like idle.
			this.trail = 0;
			this.heldColour = undefined;
			this.stopCollapse?.();
			this.stopCollapse = undefined;
		} else {
			const delta = after.length - before.length;
			if (delta > 0) {
				if (this.trail === 0) {
					// Hold the colour the blink currently shows, and start watching for idle.
					this.heldColour = CYCLE[Math.floor(this.phase / 2) % CYCLE.length]!;
					this.stopCollapse = onTick(() => this.collapse(), COLLAPSE_MS);
				}
				this.trail += delta;
			} else if (delta < 0) {
				this.trail = Math.max(0, this.trail + delta);
				if (this.trail === 0) {
					this.heldColour = undefined;
					this.stopCollapse?.();
					this.stopCollapse = undefined;
				}
			}
		}
		this.lastKeystrokeAt = Date.now();
		this.tui?.requestRender?.();
		return result;
	}

	/** The idle collapse (hs-d676): one step to zero, the blink resumes visible in the held colour. */
	private collapse(): void {
		if (Date.now() - this.lastKeystrokeAt < IDLE_MS) return;
		this.trail = 0;
		this.heldColour = undefined;
		// Make the resumed blink's phase even, so it shows the colour it held.
		this.phase -= this.phase % 2;
		this.stopCollapse?.();
		this.stopCollapse = undefined;
		this.tui?.requestRender?.();
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
		if (!this.stopTick) {
			// Start lazily on the first render while focused: one phase per tick, the render reads it.
			this.stopTick = onTick(() => {
				// The cursor stays solid while a keystroke trail is held.
				if (this.trail > 0) return;
				this.phase += 1;
				tui?.requestRender?.();
			}, PERIOD_MS);
		}
		if (this.trail > 0 && this.heldColour !== undefined) {
			return highlight(paintMarkers(rows, this.heldColour), this.heldColour, this.trail);
		}
		if (this.phase % 2 !== 0) return stripMarkers(rows);
		const index = CYCLE[Math.floor(this.phase / 2) % CYCLE.length]!;
		return paintMarkers(rows, index);
	}
}

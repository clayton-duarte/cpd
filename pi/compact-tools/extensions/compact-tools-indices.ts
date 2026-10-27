import type { ThemeColor } from "@earendil-works/pi-coding-agent";
import { italic } from "../../lib/italic.ts";
import { pill as sharedPill } from "../../lib/pill.ts";
import { indicatorGlyph, type RowStatus } from "./compact-tools-core.ts";

export { foreground } from "../../lib/pill.ts";

/**
 * The only colors compact-tools draws: indices 0-15 of the terminal's palette (docs/contract.md
 * 6.1.1, hs-d534), written as indexed SGR and never as RGB, whatever theme Pi runs. What an index
 * looks like is the terminal's theme, not named here. No faint, no mixing, no ramps.
 */
export const INDEX = {
	black: 0, red: 1, green: 2, yellow: 3, blue: 4, magenta: 5, cyan: 6, grey: 7,
	darkGrey: 8, brightWhite: 15,
} as const;

/** Rails, connectors, separators and completion metadata. */
export const RAIL = INDEX.darkGrey;
/** The running dot and the working label. */
export const RUNNING = INDEX.cyan;
/** Fold toggles and read-only text. */
export const TEXT = INDEX.grey;

/** A rail fold's body row under fold "toggle": the rail in column 0, one blank (docs/contract.md 6.1.1 "The rail fold"). */
export const RAIL_TOGGLE = "│ ";
/** The blank columns a rail fold's rows keep at the right under fold "toggle". */
export const TOGGLE_RESERVE = 2;

/** The foreground SGR parameter for an index: 30-37 below 8, 90-97 from 8. */
function sgrParam(index: number): number {
	return index < 8 ? 30 + index : 90 + index - 8;
}

/** The escape that starts painting in palette index `index`. */
export function fgOpen(index: number): string {
	return `\x1b[${sgrParam(index)}m`;
}

/** Paint `text` in palette index `index`, then return to the default foreground. */
export function paint(index: number, text: string): string {
	return `\x1b[${sgrParam(index)}m${text}\x1b[39m`;
}

/** A painter for one index, for text painted many times. */
export function painter(index: number): (text: string) => string {
	const open = `\x1b[${sgrParam(index)}m`;
	return (text) => `${open}${text}\x1b[39m`;
}

/** The index of each Pi theme token compact-tools names; one that is not listed is read-only text. */
const TOKEN_INDEX: Partial<Record<ThemeColor, number>> = {
	dim: RAIL, muted: RAIL, border: RAIL, borderMuted: RAIL, toolDiffContext: RAIL,
	mdHr: RAIL, mdQuoteBorder: RAIL, mdQuote: RAIL, mdListBullet: RAIL, mdLinkUrl: RAIL, mdCodeBlockBorder: RAIL,
	success: INDEX.green, toolDiffAdded: INDEX.green,
	error: INDEX.red, toolDiffRemoved: INDEX.red,
	warning: INDEX.yellow,
	accent: INDEX.cyan, mdCode: INDEX.cyan,
	mdLink: INDEX.blue,
	bashMode: INDEX.green,
};

/** The palette index a Pi theme token stands for in compact-tools. */
export function tokenIndex(token: ThemeColor): number {
	return TOKEN_INDEX[token] ?? TEXT;
}

/** Paint `text` as the named Pi theme token would, but as an index 0-15. */
export function ink(token: ThemeColor, text: string): string {
	return paint(tokenIndex(token), text);
}

/** Paint frame rails, connectors, separators, completion metadata and nested counts. */
export const chrome = painter(RAIL);

/** A row's status dot: running (and not yet started) cyan, done green, failed red. */
export function paintIndicator(status: RowStatus, glyph = indicatorGlyph(status)): string {
	return paint(status === "success" ? INDEX.green : status === "error" ? INDEX.red : RUNNING, glyph);
}

const statusIndex = (status: RowStatus): number =>
	status === "success" ? INDEX.green : status === "error" ? INDEX.red : status === "asking" ? INDEX.magenta : RUNNING;

/** A pill: caps in the colour on the terminal background, the text reversed (in the terminal's background). */
export function pill(index: number, text: string): string {
	return sharedPill(index, text);
}

/** A tool row's title pill: running (and not yet started) cyan, done green, failed red. */
export function statusPill(status: RowStatus, text: string): string {
	return pill(statusIndex(status), text);
}

/** The Thinking title pill, read-only grey 7. */
export function thinkingPill(text: string): string {
	return pill(INDEX.grey, italic(text));
}

/** The one place for the label's literal. */
export const THINKING_LABEL = "Thinking…";

/** Working words: italic only when they are exactly the Thinking label (hs: italic label only). */
export function workingWords(words: string): string {
	return words === THINKING_LABEL ? italic(words) : words;
}

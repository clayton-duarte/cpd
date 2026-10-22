/**
 * Chat balloons (docs/contract.md section 6.9, "Chat balloons"): pure drawing of a message's wrapped text rows.
 * Borders and tails are colour 8 (bright black, indexed SGR 90); the text rows keep their own codes.
 */
import { truncateToWidth, visibleWidth } from "@earendil-works/pi-tui";

export type BalloonSide = "user" | "agent";

/** The narrowest pane that holds a balloon: 1 column of text plus 2 walls, 2 paddings and 4 margins. */
export const MIN_BALLOON_WIDTH = 9;

const line = (text: string) => `\x1b[90m${text}\x1b[39m`;

/**
 * Draw `rows` (already wrapped to at most `width - 8` columns) as a balloon: the user's one column off the
 * right edge of the pane, the agent's one column off the left. A pane narrower than 9 columns draws no balloon.
 */
export function drawBalloon(rows: string[], side: BalloonSide, width: number): string[] {
	if (width < MIN_BALLOON_WIDTH) return rows;
	const inner = Math.min(Math.max(0, ...rows.map(visibleWidth)), width - 8);
	const box = inner + 4;
	const fit = (row: string) => {
		const cut = truncateToWidth(row, inner, "");
		return cut + " ".repeat(inner - visibleWidth(cut));
	};
	const edge = line("│");
	const body = rows.map(fit).map((row) => `${edge} ${row} ${edge}`);
	const top = line(`╭${"─".repeat(box - 2)}╮`);
	if (side === "agent") return [top, ...body, line(`│╭${"─".repeat(box - 3)}╯`), line("╰╯")].map((row) => " " + row);
	const margin = " ".repeat(width - box - 1);
	return [top, ...body, line(`╰${"─".repeat(box - 3)}╮│`), " ".repeat(box - 2) + line("╰╯")].map((row) => margin + row + " ");
}

/**
 * Chat balloons (docs/contract.md section 6.9, "Chat balloons"): pure drawing of a message's wrapped text rows.
 * Borders and tails are colour 8 (bright black, indexed SGR 90); the text rows keep their own codes.
 */
import { truncateToWidth, visibleWidth } from "@earendil-works/pi-tui";
import { localTime } from "../../lib/time.ts";

export type BalloonSide = "user" | "agent";

/** The narrowest pane that holds a balloon: 1 column of text plus 2 walls, 2 paddings and 4 columns of gap. */
export const MIN_BALLOON_WIDTH = 9;

/** The columns a balloon's text row gives up beside the walls and paddings: `width - gap` is the widest text. */
export function balloonGap(side: BalloonSide, margins = true): number {
	return side === "agent" || !margins ? 8 : 7;
}

const line = (text: string) => `\x1b[90m${text}\x1b[39m`;

/**
 * Draw `rows` (already wrapped to at most `width - 8` columns for the agent, `width - 7` for the user) as a balloon:
 * the user's ends in the pane's last column, the agent's is preceded by one blank row. A pane narrower than 9 columns
 * draws no balloon.
 * `margins` false (a pane that is not compact style with fold `toggle`) keeps the earlier geometry: one column of
 * right margin for the user, no blank row above the agent's, text rows at most `width - 8` wide for both.
 * `at` (Unix ms) is the agent balloon's machine-local time (hs-d637); a border too narrow for it (a box under
 * 9 columns) or no `at` leaves the border unchanged.
 */
export function drawBalloon(rows: string[], side: BalloonSide, width: number, at?: number, margins = true): string[] {
	if (width < MIN_BALLOON_WIDTH) return rows;
	const inner = Math.min(Math.max(0, ...rows.map(visibleWidth)), width - balloonGap(side, margins));
	const box = inner + 4;
	const fit = (row: string) => {
		const cut = truncateToWidth(row, inner, "");
		return cut + " ".repeat(inner - visibleWidth(cut));
	};
	const edge = line("│");
	const body = rows.map(fit).map((row) => `${edge} ${row} ${edge}`);
	const top = line(`╭${"─".repeat(box - 2)}╮`);
	if (side === "agent") {
		const time = at !== undefined && Number.isFinite(at) ? localTime(new Date(at).toISOString()) : undefined;
		const bottom = time !== undefined && box >= 9
			? line(`│╭─${time}${"─".repeat(box - 9)}╯`)
			: line(`│╭${"─".repeat(box - 3)}╯`);
		return [...(margins ? [""] : []), top, ...body, bottom, line("╰╯")];
	}
	const margin = " ".repeat(width - box - (margins ? 0 : 1));
	return [top, ...body, line(`╰${"─".repeat(box - 3)}╮│`), " ".repeat(box - 2) + line("╰╯")].map((row) => margin + row);
}

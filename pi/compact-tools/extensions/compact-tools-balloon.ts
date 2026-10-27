/**
 * Chat balloons (docs/contract.md section 6.9, "Chat balloons"): pure drawing of a message's wrapped text rows.
 * Borders and tails are colour 8 (bright black, indexed SGR 90); the text rows keep their own codes.
 */
import { truncateToWidth, visibleWidth } from "@earendil-works/pi-tui";
import { localTime } from "../../lib/time.ts";

export type BalloonSide = "user" | "agent";

/** The narrowest pane that holds a balloon: 1 column of text plus 2 walls, 2 paddings and 4 columns of gap. */
export const MIN_BALLOON_WIDTH = 9;

const line = (text: string) => `\x1b[90m${text}\x1b[39m`;

/**
 * Draw `rows` (already wrapped to at most `width - 8` columns for the agent, `width - 7` for the user) as a balloon:
 * the user's ends in the pane's last column, the agent's is preceded by one blank row. A pane narrower than 9 columns
 * draws no balloon.
 * `at` (Unix ms) is the agent balloon's machine-local time (hs-d637); a border too narrow for it (a box under
 * 9 columns) or no `at` leaves the border unchanged.
 */
export function drawBalloon(rows: string[], side: BalloonSide, width: number, at?: number): string[] {
	if (width < MIN_BALLOON_WIDTH) return rows;
	const inner = Math.min(Math.max(0, ...rows.map(visibleWidth)), width - (side === "agent" ? 8 : 7));
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
		return ["", top, ...body, bottom, line("╰╯")];
	}
	const margin = " ".repeat(width - box);
	return [top, ...body, line(`╰${"─".repeat(box - 3)}╮│`), " ".repeat(box - 2) + line("╰╯")].map((row) => margin + row);
}

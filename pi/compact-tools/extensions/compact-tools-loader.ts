/**
 * Pi's loader: the eight moon phases, new moon to waning crescent, one frame every 100 ms, drawn in 6 (cyan, agent
 * activity). It differs from the cockpit's loader (tui/, nine frames that fill a disc then snap back to empty, which is
 * not a loop): the User chose the moon phases on 2026-10-22, so the last frame leads into the first with no repeat.
 */
import { INDEX, paint } from "./compact-tools-indices.ts";

export const LOADER_FRAMES: readonly string[] = [
	0xf0f64, // moon_new
	0xf0f67, // moon_waxing_crescent
	0xf0f61, // moon_first_quarter
	0xf0f68, // moon_waxing_gibbous
	0xf0f62, // moon_full
	0xf0f66, // moon_waning_gibbous
	0xf0f63, // moon_last_quarter
	0xf0f65, // moon_waning_crescent
].map((code) => String.fromCodePoint(code));
export const LOADER_INTERVAL_MS = 100;

/** The frame shown at `now` (ms), painted in 6. */
export function loaderFrame(now: number = Date.now()): string {
	return paint(INDEX.cyan, LOADER_FRAMES[Math.floor(now / LOADER_INTERVAL_MS) % LOADER_FRAMES.length]!);
}

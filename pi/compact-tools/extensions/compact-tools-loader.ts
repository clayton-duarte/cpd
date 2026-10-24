/**
 * Pi's loader: the 28 moon_alt phases (Nerd Fonts 3.5.1), new moon to waning crescent, one frame every 50 ms (one turn
 * in 1.4 s), drawn in 6 (cyan, agent activity). The User chose them on 2026-10-24 (hs-d616, a trial); the cockpit has
 * the same 28 frames at 50 ms (tui/src/palette.rs). The last frame leads into the first with no repeat.
 */
import { INDEX, paint } from "./compact-tools-indices.ts";

export const LOADER_FRAMES: readonly string[] = [
	0xe3e3, // moon_alt_new
	0xe3c8, // moon_alt_waxing_crescent_1
	0xe3c9, // moon_alt_waxing_crescent_2
	0xe3ca, // moon_alt_waxing_crescent_3
	0xe3cb, // moon_alt_waxing_crescent_4
	0xe3cc, // moon_alt_waxing_crescent_5
	0xe3cd, // moon_alt_waxing_crescent_6
	0xe3ce, // moon_alt_first_quarter
	0xe3cf, // moon_alt_waxing_gibbous_1
	0xe3d0, // moon_alt_waxing_gibbous_2
	0xe3d1, // moon_alt_waxing_gibbous_3
	0xe3d2, // moon_alt_waxing_gibbous_4
	0xe3d3, // moon_alt_waxing_gibbous_5
	0xe3d4, // moon_alt_waxing_gibbous_6
	0xe3d5, // moon_alt_full
	0xe3d6, // moon_alt_waning_gibbous_1
	0xe3d7, // moon_alt_waning_gibbous_2
	0xe3d8, // moon_alt_waning_gibbous_3
	0xe3d9, // moon_alt_waning_gibbous_4
	0xe3da, // moon_alt_waning_gibbous_5
	0xe3db, // moon_alt_waning_gibbous_6
	0xe3dc, // moon_alt_third_quarter
	0xe3dd, // moon_alt_waning_crescent_1
	0xe3de, // moon_alt_waning_crescent_2
	0xe3df, // moon_alt_waning_crescent_3
	0xe3e0, // moon_alt_waning_crescent_4
	0xe3e1, // moon_alt_waning_crescent_5
	0xe3e2, // moon_alt_waning_crescent_6
].map((code) => String.fromCodePoint(code));
export const LOADER_INTERVAL_MS = 50;

/** The frame shown at `now` (ms), painted in 6. */
export function loaderFrame(now: number = Date.now()): string {
	return paint(INDEX.cyan, LOADER_FRAMES[Math.floor(now / LOADER_INTERVAL_MS) % LOADER_FRAMES.length]!);
}

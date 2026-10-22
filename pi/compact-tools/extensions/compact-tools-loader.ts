/**
 * The cockpit's loader (docs/commands.md, tui/src/palette.rs): nine frames, U+F0766 (nf-md-circle_outline) then
 * U+F0A9E .. U+F0AA5 (nf-md-circle_slice_1 .. 8), one frame every 100 ms, drawn in 6 (cyan, agent activity).
 */
import { INDEX, paint } from "./compact-tools-indices.ts";

export const LOADER_FRAMES: readonly string[] = [
	"\u{F0766}",
	...Array.from({ length: 8 }, (_, step) => String.fromCodePoint(0xf0a9e + step)),
];
export const LOADER_INTERVAL_MS = 100;

/** The frame shown at `now` (ms), painted in 6. */
export function loaderFrame(now: number = Date.now()): string {
	return paint(INDEX.cyan, LOADER_FRAMES[Math.floor(now / LOADER_INTERVAL_MS) % LOADER_FRAMES.length]!);
}

/**
 * The pi loader's eight frames, one literal list for the tests (written as codepoints, NOT imported from the
 * extension: the tests pin the contract). The moon phases, new moon to waning crescent, nf-md-moon_*.
 * What changed: the cockpit's nine frames (U+F0766, then U+F0A9E..U+F0AA5) filled and snapped; pi's loader is now a loop.
 */
export const MOON_FRAMES: readonly string[] = [0xf0f64, 0xf0f67, 0xf0f61, 0xf0f68, 0xf0f62, 0xf0f66, 0xf0f63, 0xf0f65]
	.map((code) => String.fromCodePoint(code));

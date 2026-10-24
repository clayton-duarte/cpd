/**
 * The pi loader's 28 frames, one literal list for the tests (written as codepoints, NOT imported from the
 * extension: the tests pin the contract). The moon_alt phases of Nerd Fonts 3.5.1: U+E3E3 (new), then U+E3C8..U+E3E2
 * (waxing crescent, first quarter, waxing gibbous, full, waning gibbous, third quarter, waning crescent).
 * What changed: the eight nf-md moons (U+F0F64 ...) at 100 ms became these 28 at 50 ms (hs-d616).
 */
const CODES = [0xe3e3, ...Array.from({ length: 27 }, (_, i) => 0xe3c8 + i)];
export const MOON_FRAMES: readonly string[] = CODES.map((code) => String.fromCodePoint(code));

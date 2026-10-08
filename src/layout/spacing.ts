/** Spacing language: 1rem is the height of the text and the unit of spacing.
 *  These MUST stay in sync with the --space-* tokens in src/theme.css.
 *  ELK takes plain numbers and cannot read CSS custom properties, which is
 *  why the values are mirrored here instead of referenced. */
export const REM = 16;
export const SPACE_CARDS = 2 * REM; // 32px, --space-cards
export const PAD = 0.75 * REM; // 12px, --pad

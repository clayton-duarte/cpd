/** pi's theme JSON has colours only; cpd italicises by SGR 3 (contract 6.1.1). */
export const ITALIC_ON = "\x1b[3m";
export const ITALIC_OFF = "\x1b[23m";
export const italic = (text: string): string => `${ITALIC_ON}${text}${ITALIC_OFF}`;

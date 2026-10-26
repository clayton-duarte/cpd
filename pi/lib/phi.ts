/**
 * cpd's mark φ is always bold wherever it renders (contract §6.1.1 rule (4)): bold is SGR 1
 * to open and SGR 22 to close, indexed only, never SGR 2 and never RGB. Only the glyph is bold; the
 * colon and any text that follows it are not. SGR codes are zero-width, so wrapping does not change
 * the visible text or its width.
 */
export function boldPhi(text: string): string {
  return text.replaceAll("φ", "\x1b[1mφ\x1b[22m");
}

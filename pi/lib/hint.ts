import { Theme } from "@earendil-works/pi-coding-agent";

// Contract §6.1.1 Palette: a hint is text only, colour 8, no background.
// pi draws its jump hint as theme.bg("selectedBg", theme.fg("text", label)); a theme alone does not reach it.
const KEY = Symbol.for("cpd.hint.original");
const HINT = /^ ↓ Jump to latest message( · \S+)? $/;
// The question widget's single-question header (@juicesharp/rpiv-ask-user-question view/tab-content-strategy.ts:112) is
// theme.bg("selectedBg", ` ${header} `): plain text, one space at each end. Contract §6.1.1: a title is a read-only grey pill, no padding.
// Recognised by that shape alone, not by call site: the other selectedBg callers (selectors, tab bar, widget list rows) always
// pass ANSI-bearing text (fg/bold wrapped), which this never matches; a stack lookup would also be fragile across bundlers.
// Grey 7 title pill in the order the red test and contract 6.1.1 give: text, then ESC[49m, then ESC[39m (the tool pill painter
// of compact-tools-indices.ts resets in the other order, which its own tests pin; the two are equivalent on screen).
const titlePill = (text: string): string => `\x1b[37m\uE0B6\x1b[39m\x1b[47m\x1b[30m${text}\x1b[49m\x1b[39m\x1b[37m\uE0B4\x1b[39m`;
const HEADER = /^ ([^\s\x1b](?:[^\x1b]*[^\s\x1b])?) $/;
// eslint-disable-next-line no-control-regex
const SGR = /\x1b\[[0-9;]*m/g;

type Bg = (this: unknown, color: string, text: string) => string;
type Holder = { prototype: { bg: Bg; [KEY]?: Bg } };

/** Wraps `Theme.prototype.bg` for the jump hint and the question header; idempotent. Returns the removal. */
export function installHint(): () => void {
  const proto = (Theme as unknown as Holder).prototype;
  if (!proto[KEY]) {
    const original = proto.bg;
    proto[KEY] = original;
    proto.bg = function (this: unknown, color: string, text: string): string {
      if (color === "selectedBg") {
        const plain = text.replace(SGR, "");
        if (HINT.test(plain)) return `\x1b[90m${plain}\x1b[39m`;
        const header = HEADER.exec(text);
        if (header) return titlePill(header[1]!);
      }
      return original.call(this, color, text);
    };
  }
  return () => {
    const original = proto[KEY];
    if (original) {
      proto.bg = original;
      delete proto[KEY];
    }
  };
}

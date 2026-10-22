import { Theme } from "@earendil-works/pi-coding-agent";

// Contract §6.1.1 Palette: a hint is text only, colour 8, no background.
// pi draws its jump hint as theme.bg("selectedBg", theme.fg("text", label)); a theme alone does not reach it.
const KEY = Symbol.for("cpd.hint.original");
const HINT = /^ ↓ Jump to latest message( · \S+)? $/;
// eslint-disable-next-line no-control-regex
const SGR = /\x1b\[[0-9;]*m/g;

type Bg = (this: unknown, color: string, text: string) => string;
type Holder = { prototype: { bg: Bg; [KEY]?: Bg } };

/** Wraps `Theme.prototype.bg` for the jump hint only; idempotent. Returns the removal. */
export function installHint(): () => void {
  const proto = (Theme as unknown as Holder).prototype;
  if (!proto[KEY]) {
    const original = proto.bg;
    proto[KEY] = original;
    proto.bg = function (this: unknown, color: string, text: string): string {
      if (color === "selectedBg") {
        const plain = text.replace(SGR, "");
        if (HINT.test(plain)) return `\x1b[90m${plain}\x1b[39m`;
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

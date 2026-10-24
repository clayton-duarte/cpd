import { Theme } from "@earendil-works/pi-coding-agent";
import { Container, truncateToWidth } from "@earendil-works/pi-tui";
import { meaning, pill } from "./pill.ts";

// Contract §6.1.1 (the Dango palette): a hint is text only, colour 8, no background.
// pi draws its jump hint as theme.bg("selectedBg", theme.fg("text", label)); a theme alone does not reach it.
const KEY = Symbol.for("cpd.hint.original");
const HINT = /^ ↓ Jump to latest message( · \S+)? $/;
// The question widget's single-question header (@juicesharp/rpiv-ask-user-question view/tab-content-strategy.ts:112) is
// theme.bg("selectedBg", ` ${header} `): plain text, one space at each end. Contract §6.1.1: a title is a read-only grey pill, no padding.
// Recognised by that shape alone, not by call site: the other selectedBg callers (selectors, tab bar, widget list rows) always
// pass ANSI-bearing text (fg/bold wrapped), which this never matches; a stack lookup would also be fragile across bundlers.
// Grey 7 title pill: caps in the colour, the text reversed (the terminal's background on the colour).
const titlePill = (text: string): string => pill(meaning("plain"), text);
const HEADER = /^ ([^\s\x1b](?:[^\x1b]*[^\s\x1b])?) $/;
// eslint-disable-next-line no-control-regex
const SGR = /\x1b\[[0-9;]*m/g;

type Bg = (this: unknown, color: string, text: string) => string;
type Holder = { prototype: { bg: Bg; [KEY]?: Bg } };

// The question widget's multi-question tab bar (view/components/tab-bar.ts `TabBar`, props `{ tabs, submit }`): its arrows and
// marks are literal text, so the line is drawn from the props. Recognised by class name and props shape only. Contract §6.1.1:
// pills joined by one colour-8 `─`; current blue 4, answered green 2, else grey 7; Submit blue when current, green when all answered.
const TAB_KEY = Symbol.for("cpd.tabbar.original");
type Tab = { label: string; answered: boolean; active: boolean };
type TabBarLike = { props: { tabs: ReadonlyArray<Tab>; submit: { active: boolean; allAnswered: boolean } }; render(width: number): string[] };
const isTabBar = (c: unknown): c is TabBarLike => {
  const o = c as { constructor?: { name?: string }; props?: { tabs?: unknown; submit?: unknown } } | null;
  return o?.constructor?.name === "TabBar" && Array.isArray(o.props?.tabs) && typeof o.props?.submit === "object" && o.props.submit !== null;
};
const SEPARATOR = "\x1b[90m─\x1b[39m";
function tabLine(bar: TabBarLike, width: number): string[] {
  const { tabs, submit } = bar.props;
  const pills = tabs.map((t) => pill(meaning(t.active ? "focus" : t.answered ? "done" : "plain"), t.label));
  pills.push(pill(meaning(submit.active ? "focus" : submit.allAnswered ? "done" : "plain"), "Submit"));
  return [truncateToWidth(` ${pills.join(SEPARATOR)}`, width, ""), ""];
}

type Render = (this: Container, width: number) => string[];
type ContainerProto = { render: Render; [TAB_KEY]?: Render };

/** Wraps `Theme.prototype.bg` for the jump hint and the question header, and `Container.prototype.render` for the tab bar; idempotent. Returns the removal. */
export function installHint(): () => void {
  const container = Container.prototype as unknown as ContainerProto;
  if (!container[TAB_KEY]) {
    const original = container.render;
    container[TAB_KEY] = original;
    container.render = function (this: Container, width: number): string[] {
      const shadowed = this.children.filter((c) => isTabBar(c)) as unknown as TabBarLike[];
      for (const bar of shadowed) (bar as { render: unknown }).render = (w: number) => tabLine(bar, w);
      try {
        return original.call(this, width);
      } finally {
        for (const bar of shadowed) delete (bar as { render?: unknown }).render;
      }
    };
  }
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
    const tabOriginal = container[TAB_KEY];
    if (tabOriginal) {
      container.render = tabOriginal;
      delete container[TAB_KEY];
    }
    const original = proto[KEY];
    if (original) {
      proto.bg = original;
      delete proto[KEY];
    }
  };
}

import { type Component, MouseRegion, sliceByColumn, stripTerminalSequences, truncateToWidth, visibleWidth, wrapTextWithAnsi } from "@earendil-works/pi-tui";
import { loadConfig } from "../compact-tools/extensions/compact-tools-config.ts";
import type { DisplayStyle } from "../compact-tools/extensions/compact-tools-types.ts";
import { meaning, pill as drawPill } from "./pill.ts";
import { boldPhi } from "./phi.ts";
import { isObject } from "./cpd.ts";
import { localTime } from "./time.ts";
import { DECISION_TYPE, SUMMARY_TYPE } from "./plan-review.ts";
import type { MessageRenderer, RenderedMessage, Theme } from "./pi.ts";
import { SKILLS_TYPE } from "./skills.ts";
import { CONTEXT_TYPE, countsOf, summaryLine } from "./updates.ts";

/**
 * Every cpd message in a pi pane is one fold, drawn like every block that folds: a title row, `╭─` then the title as a pill: the left cap, the title, the right cap, no padding inside, no dot (contract §15, the fold language). It matches the cockpit's pill title, `title` in `tui/src/section.rs` (what `tui/src/dialog/view.rs` and `tui/src/logs/view.rs` test): the lead `╭─` muted, the text REVERSED. pi's theme has no inverse, so the caps are `theme.fg("muted")` and the text is muted then SGR-reversed (the muted colour becomes its background). Expanded, each
 * content line led by a muted `│ ` and a muted `╰󰬬 collapse` line; folded, a muted `╰󰬦 expand` line (U+F0B2C and U+F0B26, the
 * cockpit's own toggle, `toggle` in `tui/src/fold.rs`; glyphs `UP` / `DOWN` in `tui/src/tree/look.rs`; contract §5.3). Colours come from pi's theme (`muted`, bold).
 * Expanding is pi's own mechanism, the renderer's `expanded` option (Ctrl+O), plus a left click anywhere on the message: pi gives the
 * renderer one mouse region per message, so the click cannot be limited to the toggle line. Display only: the message's `content` and
 * `details` are read, never changed. The style `off` (pi-compact-tools' config) still hands the message to pi's own box.
 */

/** Every `customType` the adapter sends with `display: true`. */
export const RENDERED_TYPES: readonly string[] = ["cpd", CONTEXT_TYPE, DECISION_TYPE, SUMMARY_TYPE, SKILLS_TYPE];

// ---- the style: the extension's effective config, read once ----

/** The extension's `DisplayStyle`: one source of truth, so a file, a default and a warning exist once (contract §6.9). */
export type Style = DisplayStyle;

/** `loadConfig` of `compact-tools-config.ts`: pi's agent directory (`getAgentDir()`, `PI_CODING_AGENT_DIR`), then (a trusted project only) `<cwd>/.pi/`, over its defaults. */
export function loadStyle(project: { cwd: string; trusted: boolean }): Style {
  return loadConfig(project.cwd, project.trusted).style;
}

// ---- what a message says ----

/** The message's text as pi hands it: a string, or blocks. */
function contentOf(message: RenderedMessage): string {
  const c = message.content;
  if (typeof c === "string") return c;
  if (Array.isArray(c)) return c.map((b) => (isObject(b) && b.type === "text" && typeof b.text === "string" ? b.text : "")).filter(Boolean).join("\n");
  return String(c);
}

/** `CUSTOM_SUMMARY_MAX_LENGTH` of `compact-tools-invocation.ts`. */
const SUMMARY_MAX = 160;

/** One line: the first non-blank one, controls and runs of whitespace as one space, cut with `…` as the extension cuts a custom tool's summary. */
function oneLine(text: string): string {
  const first = stripTerminalSequences(text.replace(/\r\n?/g, "\n")).split("\n").find((l) => l.trim() !== "") ?? "";
  const line = first.replace(/[\x00-\x1f\x7f]/gu, " ").replace(/\s+/gu, " ").trim();
  return line.length > SUMMARY_MAX ? `${line.slice(0, SUMMARY_MAX - 1).trimEnd()}…` : line;
}

/** `HH:MM` in the process's local time for a Unix-ms `timestamp`, or `undefined` when there is none. */
function timeOf(timestamp: number | undefined): string | undefined {
  return typeof timestamp === "number" && Number.isFinite(timestamp) ? localTime(new Date(timestamp).toISOString()) : undefined;
}

/** `Message from <name> (<id>) (<time>): …` or `Message from <id> (<time>): …` (delivery.ts `sessionMessage`): the sender's name, or its id. */
function senderOf(from: string | null, content: string): string {
  if (from === null) return "the User";
  const rest = content.startsWith("Message from ") ? content.slice("Message from ".length) : "";
  if (rest.startsWith(`${from} `) || rest.startsWith(`${from}: `)) return from;
  const named = rest.indexOf(` (${from})`);
  return named > 0 ? rest.slice(0, named) : from;
}

const ADAPTER_PREFIX = "φ adapter: ";

/** commands.md §15.9 "The summary per kind". `undefined`: nothing short to say (a `cpd-context` without counts). */
function summaryOf(message: RenderedMessage, content: string): string | undefined {
  const d = isObject(message.details) ? message.details : {};
  if (message.customType === CONTEXT_TYPE) {
    if (typeof d.summary === "string") return d.summary;
    const counts = countsOf(message.details);
    return counts === undefined ? undefined : summaryLine(counts);
  }
  if (message.customType === SUMMARY_TYPE) {
    return typeof d.review === "string" ? `Plan review ${d.review} summary` : oneLine(content);
  }
  if (message.customType === DECISION_TYPE) {
    if (typeof d.review === "string" && typeof d.approved === "boolean") return `Plan review ${d.review} ${d.approved ? "approved" : "rejected"}`;
    return oneLine(content);
  }
  if (message.customType === "cpd") {
    // The adapter's own error has no Event `id`; an `error` Event has one.
    if (d.kind === "error" && d.id === undefined && typeof d.code === "string" && content.startsWith(ADAPTER_PREFIX)) {
      const rest = content.slice(ADAPTER_PREFIX.length);
      const at = rest.indexOf(`: ${d.code}: `);
      return oneLine(`adapter: ${at >= 0 ? rest.slice(0, at) : rest}`);
    }
    if (d.kind === "session-message" && isObject(d.data)) return oneLine(`Message from ${senderOf(typeof d.data.from === "string" ? d.data.from : null, content)}`);
    if (typeof d.text === "string") return oneLine(d.text);
  }
  return oneLine(content);
}

// ---- the extension's layout helpers ----

const normalizeLineEndings = (value: string): string => value.replace(/\r\n?|\n/g, "\n");

/** `hardWrapTextWithAnsi` of `compact-tools-layout.ts`. */
function hardWrap(text: string, width: number): string[] {
  const safeWidth = Math.max(1, width);
  const wrapped: string[] = [];
  for (const logicalLine of text.replace(/\t/g, "   ").split("\n")) {
    const lineWidth = visibleWidth(logicalLine);
    if (lineWidth === 0) {
      wrapped.push("");
      continue;
    }
    let offset = 0;
    while (offset < lineWidth) {
      if (offset > 0) {
        const remainder = sliceByColumn(logicalLine, offset, lineWidth - offset, true);
        const whitespace = stripTerminalSequences(remainder).match(/^\s+/u)?.[0] ?? "";
        offset += visibleWidth(whitespace);
        if (offset >= lineWidth) break;
      }
      wrapped.push(sliceByColumn(logicalLine, offset, safeWidth, true));
      offset += safeWidth;
    }
  }
  return wrapped;
}

/** `prefixedText`: word-wrapped under a prefix. */
function prefixedText(text: string, first: string, rest: string = first): (width: number) => string[] {
  const prefixWidth = Math.max(visibleWidth(first), visibleWidth(rest));
  const normalized = text.replace(/\t/g, "   ");
  return (width) => wrapTextWithAnsi(normalized, Math.max(1, width - prefixWidth)).map((line, i) => `${i === 0 ? first : rest}${line}`);
}

/** `WithinWidth`: a row never draws past the pane's edge. */
const withinWidth = (lines: string[], width: number): string[] => lines.map((l) => (visibleWidth(l) > width ? truncateToWidth(l, Math.max(1, width), "") : l));

// ---- the fold ----

const GREY8 = (text: string): string => `\x1b[90m${text}\x1b[39m`;

/** The text up to the first `.`, `:` or line end, cut at 60 characters with `…` only when longer. */
function firstSentence(summary: string): string {
  const line = (summary.split("\n")[0] ?? "").trim();
  const at = line.search(/[.:]/);
  const s = at >= 0 && at < line.length - 1 ? line.slice(0, at).trim() : line;
  return s.length > 60 ? `${s.slice(0, 60)}…` : s;
}
const TOP = "╭─";
const RAIL = "│ ";
const CLOSE = "╰";
/** U+F0B2A, the 'nothing to fold' glyph; the same code point as compact-tools' own constant in `compact-tools/extensions/compact-tools-layout.ts` (not imported across the two trees). */
const NOTHING_TO_FOLD = "\u{F0B2A}";
const DOWN = "\u{F0B26}";
const UP = "\u{F0B2C}";
const EXPAND = "expand";
const COLLAPSE = "collapse";

type Rows = { foldable: boolean; terminator: () => string[]; head: (width: number) => string[]; body: (width: number) => string[]; toggle: (width: number, expanded: boolean) => string[] };

/** The fold's rows for one message. `body` is empty when `content` is. */
function rowsOf(summary: string, content: string, theme: Theme, time?: string): Rows {
  void theme;
  const text = normalizeLineEndings(content).trimEnd();
  const lead = GREY8(TOP);
  // contract §6.1.1: the pill is magenta 5 inverted, caps 5, by explicit indexed SGR; the sentence follows it outside, text 7.
  const pill = drawPill(meaning("planning"), boldPhi("φ"));
  const sentence = firstSentence(summary);
  const rail = GREY8(RAIL);
  const body = text === "" ? "" : text.split("\n").map((l) => (l === "" ? l : `\x1b[37m${boldPhi(l)}\x1b[39m`)).join("\n");
  return {
    head: (width) => {
      const room = Math.max(1, width - visibleWidth(lead) - visibleWidth(pill) - 1);
      const lines = wrapTextWithAnsi(sentence, room);
      return (lines.length === 0 ? [""] : lines).map((l, i) => {
        const t = l === "" ? "" : `\x1b[37m${l}\x1b[39m`;
        return i === 0 ? `${lead}${pill}${t === "" ? "" : ` ${t}`}` : `${GREY8(RAIL)}${t}`;
      });
    },
    body: text === "" ? () => [] : prefixedText(body, rail),
    foldable: text !== "",
    terminator: () => [`${GREY8(`${CLOSE}${NOTHING_TO_FOLD}`)}`],
    toggle: (width, expanded) => {
      const base = `${GREY8(CLOSE)}\x1b[37m${expanded ? UP : DOWN} ${expanded ? COLLAPSE : EXPAND}`;
      const dated = time === undefined ? base : `${base} · ${time}`;
      // A narrow pane drops the time rather than cutting it: the toggle row stays as it was.
      return [`${time !== undefined && visibleWidth(dated) > width ? base : dated}\x1b[39m`];
    },
  };
}

/** What one message remembers: whether a click (or Ctrl+O) has opened it, and the host's own `expanded` last seen. */
type OpenState = { expanded: boolean; host: boolean };

/** The one renderer of every cpd `customType`, with the style read at `session_start`. */
export class Rendering {
  style: Style = "compact";
  private readonly states = new WeakMap<RenderedMessage, OpenState>();

  /** The extension's `loadConfig`, once per `session_start` (§15.9 "Which style"). */
  load(project: { cwd: string; trusted: boolean }): void {
    this.style = loadStyle(project);
  }

  /** §15.9 `off`: no component, so pi draws its own default box with the full `content`. */
  readonly render: MessageRenderer = (message, { expanded }, theme) => {
    if (this.style === "off") return undefined;
    const content = contentOf(message);
    const summary = summaryOf(message, content);
    const rows = rowsOf(summary ?? CONTEXT_TYPE, content, theme, timeOf(message.timestamp));
    // No summary, and always the Plan review's summary, which the User reads to decide: the row is always open, with no closing row and no toggle (§15.9 "No summary").
    const open = summary === undefined || message.customType === SUMMARY_TYPE;
    const closes = open || !rows.foldable;
    const state = this.states.get(message) ?? { expanded, host: expanded };
    this.states.set(message, state);
    // Ctrl+O hands the renderer `expanded`; the message follows it both ways (the extension's group rule).
    if (state.host !== expanded) {
      state.host = expanded;
      state.expanded = expanded;
    }
    const draw = (width: number): string[] => {
      const shown = open || state.expanded;
      const lines = [...rows.head(width), ...(shown ? rows.body(width - 3) : []), ...(closes ? rows.terminator() : rows.toggle(width, shown))];
      return withinWidth(lines, width);
    };
    const inner: Component = {
      render: (width) => draw(width),
      invalidate() {},
    };
    if (closes) return inner;
    // A left click toggles this one message, as pi does a tool row (`createResultRegion`).
    return new MouseRegion(inner, (event) => {
      if (event.type !== "click" || event.button !== "left") return undefined;
      state.expanded = !state.expanded;
      return { handled: true };
    });
  };
}

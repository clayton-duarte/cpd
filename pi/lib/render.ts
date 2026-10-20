import { type Component, MouseRegion, sliceByColumn, stripTerminalSequences, truncateToWidth, visibleWidth, wrapTextWithAnsi } from "@earendil-works/pi-tui";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { isObject } from "./cpd.ts";
import { DECISION_TYPE } from "./plan-review.ts";
import type { MessageRenderer, RenderedMessage, Theme } from "./pi.ts";
import { CONTEXT_TYPE, countsOf, summaryLine } from "./updates.ts";

/**
 * Every cpd message in a pi pane is one fold, drawn like every block that folds: a title row, `╭─` then the title as a pill: U+E0B6, the title, U+E0B4, no padding inside, no dot (contract §15, the fold language). It matches the cockpit's pill title, `title` in `tui/src/section.rs` (what `tui/src/dialog/view.rs` and `tui/src/logs/view.rs` test): the lead `╭─` muted, the text REVERSED. pi's theme has no inverse, so the caps are `theme.fg("muted")` and the text is muted then SGR-reversed (the muted colour becomes its background). Expanded, each
 * content line led by a muted `│ ` and a muted `╰󰁣 collapse` line; folded, a muted `╰󰁋 expand` line (U+F0063 and U+F004B, the
 * cockpit's own toggle, `toggle` in `tui/src/fold.rs`; glyphs `UP` / `DOWN` in `tui/src/tree/look.rs`; contract §5.3). Colours come from pi's theme (`muted`, bold).
 * Expanding is pi's own mechanism, the renderer's `expanded` option (Ctrl+O), plus a left click anywhere on the message: pi gives the
 * renderer one mouse region per message, so the click cannot be limited to the toggle line. Display only: the message's `content` and
 * `details` are read, never changed. The style `off` (pi-compact-tools' config) still hands the message to pi's own box.
 */

/** Every `customType` the adapter sends with `display: true`. */
export const RENDERED_TYPES: readonly string[] = ["cpd", CONTEXT_TYPE, DECISION_TYPE];

// ---- the style: the extension's `compact-tools.json`, read once ----

/** `DISPLAY_STYLES` of `compact-tools-types.ts`. */
const STYLES = ["compact", "claude", "codex", "off"] as const;
export type Style = (typeof STYLES)[number];
/** `DEFAULT_CONFIG.style` of `compact-tools-config.ts`. */
const DEFAULT_STYLE: Style = "compact";
const CONFIG_FILE = "compact-tools.json";

/** `compact-tools-config.ts` `warn`: one line on stderr. */
const warn = (path: string, message: string): void => console.error(`[compact-tools] ${path}: ${message}`);

/** `compact-tools-config.ts` `readJson` + `mergeConfig`, for the one key that matters: a file that is bad is reported and the value before it kept. */
function styleFrom(path: string, before: Style): Style {
  if (!existsSync(path)) return before;
  let value: unknown;
  try {
    value = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    warn(path, error instanceof Error ? error.message : String(error));
    return before;
  }
  if (!isObject(value)) {
    warn(path, "expected a JSON object; using previous values");
    return before;
  }
  if (value.style === undefined) return before;
  if ((STYLES as readonly unknown[]).includes(value.style)) return value.style as Style;
  warn(path, `style must be one of ${STYLES.join(", ")}; using previous value`);
  return before;
}

/** pi's `getAgentDir()`: `PI_CODING_AGENT_DIR` (a leading `~` is the home directory), else `~/.pi/agent`. */
function agentDirOf(env: Record<string, string | undefined>): string {
  const dir = env.PI_CODING_AGENT_DIR;
  if (!dir) return join(homedir(), ".pi", "agent");
  return dir === "~" ? homedir() : dir.startsWith("~/") ? join(homedir(), dir.slice(2)) : dir;
}

/** `loadConfig` of the extension: pi's agent directory, then (a trusted project only) `<cwd>/.pi/`, which overrides it. */
export function loadStyle(env: Record<string, string | undefined>, project: { cwd: string; trusted: boolean }): Style {
  const global = styleFrom(join(agentDirOf(env), CONFIG_FILE), DEFAULT_STYLE);
  return project.trusted ? styleFrom(join(project.cwd, ".pi", CONFIG_FILE), global) : global;
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

/** `Message from <name> (<id>): …` or `Message from <id>: …` (delivery.ts `sessionMessage`): the sender's name, or its id. */
function senderOf(from: string | null, content: string): string {
  if (from === null) return "the User";
  const rest = content.startsWith("Message from ") ? content.slice("Message from ".length) : "";
  if (rest.startsWith(`${from}: `)) return from;
  const named = rest.indexOf(` (${from}): `);
  return named > 0 ? rest.slice(0, named) : from;
}

const ADAPTER_PREFIX = "cpd adapter: ";

/** commands.md §15.9 "The summary per kind". `undefined`: nothing short to say (a `cpd-context` without counts). */
function summaryOf(message: RenderedMessage, content: string): string | undefined {
  const d = isObject(message.details) ? message.details : {};
  if (message.customType === CONTEXT_TYPE) {
    const counts = countsOf(message.details);
    return counts === undefined ? undefined : summaryLine(counts);
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

/** `outputPadLines`: with `outputPad` 0 the row's one margin column is reclaimed. */
function outputPadLines(lines: string[], width: number): string[] {
  return lines.map((line) => (stripTerminalSequences(line).startsWith(" ") ? sliceByColumn(line, 1, Math.max(0, width), true) : truncateToWidth(line, Math.max(0, width), "…")));
}

/** `WithinWidth`: a row never draws past the pane's edge. */
const withinWidth = (lines: string[], width: number): string[] => lines.map((l) => (visibleWidth(l) > width ? truncateToWidth(l, Math.max(1, width), "") : l));

// ---- the fold ----

const CAP_LEFT = "\u{E0B6}";
const CAP_RIGHT = "\u{E0B4}";
const MARGIN = " ";
const TOP = "╭─";
const RAIL = "│ ";
const CLOSE = "╰";
const DOWN = "\u{F004B}";
const UP = "\u{F0063}";
const EXPAND = "expand";
const COLLAPSE = "collapse";

type Rows = { head: (width: number) => string[]; body: (width: number) => string[]; toggle: (width: number, expanded: boolean) => string[] };

/** The fold's rows for one message. `body` is empty when `content` is. */
function rowsOf(summary: string, content: string, theme: Theme): Rows {
  const muted = (text: string) => theme.fg("muted", text);
  const text = normalizeLineEndings(content).trimEnd();
  const lead = `${MARGIN}${muted(`${TOP}${CAP_LEFT}`)}`;
  const indent = " ".repeat(visibleWidth(lead));
  const pill = (line: string) => `\x1b[7m${muted(line)}\x1b[27m`;
  const rail = `${MARGIN}${muted(RAIL)}`;
  return {
    head: (width) => {
      const room = width - visibleWidth(lead);
      let lines = hardWrap(`cpd ${summary}`, room);
      // The closing cap needs its own cell on the last row.
      if (visibleWidth(lines[lines.length - 1]) + visibleWidth(CAP_RIGHT) > room) lines = hardWrap(`cpd ${summary}`, room - visibleWidth(CAP_RIGHT));
      return lines.map((l, i) => `${i === 0 ? lead : indent}${pill(l)}${i === lines.length - 1 ? muted(CAP_RIGHT) : ""}`);
    },
    body: text === "" ? () => [] : prefixedText(text, rail),
    toggle: (_width, expanded) => [`${MARGIN}${muted(`${CLOSE}${expanded ? UP : DOWN} ${expanded ? COLLAPSE : EXPAND}`)}`],
  };
}

/** What one message remembers: whether a click (or Ctrl+O) has opened it, and the host's own `expanded` last seen. */
type OpenState = { expanded: boolean; host: boolean };

/** The one renderer of every cpd `customType`, with the style read at `session_start`. */
export class Rendering {
  style: Style = DEFAULT_STYLE;
  private readonly states = new WeakMap<RenderedMessage, OpenState>();

  /** The extension's `loadConfig`, once per `session_start` (§15.9 "Which style"). */
  load(env: Record<string, string | undefined>, project: { cwd: string; trusted: boolean }): void {
    this.style = loadStyle(env, project);
  }

  /** §15.9 `off`: no component, so pi draws its own default box with the full `content`. */
  readonly render: MessageRenderer = (message, { expanded, outputPad }, theme) => {
    if (this.style === "off") return undefined;
    const content = contentOf(message);
    const summary = summaryOf(message, content);
    const rows = rowsOf(summary ?? CONTEXT_TYPE, content, theme);
    // No summary: the row is always open, with no closing row and no toggle (§15.9 "No summary").
    const open = summary === undefined;
    const state = this.states.get(message) ?? { expanded, host: expanded };
    this.states.set(message, state);
    // Ctrl+O hands the renderer `expanded`; the message follows it both ways (the extension's group rule).
    if (state.host !== expanded) {
      state.host = expanded;
      state.expanded = expanded;
    }
    const draw = (width: number): string[] => {
      const shown = open || state.expanded;
      const lines = [...rows.head(width), ...(shown ? rows.body(width) : []), ...(open ? [] : rows.toggle(width, shown))];
      return withinWidth(lines, width);
    };
    const inner: Component = {
      render: (width) => {
        const lines = outputPad === 0 ? outputPadLines(draw(width + 1), width) : draw(width);
        return lines.map((l) => theme.bg("customMessageBg", l + " ".repeat(Math.max(0, width - visibleWidth(l)))));
      },
      invalidate() {},
    };
    if (open) return inner;
    // A left click toggles this one message, as pi does a tool row (`createResultRegion`).
    return new MouseRegion(inner, (event) => {
      if (event.type !== "click" || event.button !== "left") return undefined;
      state.expanded = !state.expanded;
      return { handled: true };
    });
  };
}

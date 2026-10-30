/*
 * fold: "toggle", head half; see docs/contract.md section 6.9.
 *
 * renderToolCall: with fold "toggle" the first line's prefix is "╭─" (chrome-painted) instead of
 * " "; today's " " stays under fold "status". Continuation lines are "│ " (toggle) or " │ " (status); `reserve`
 * is the blank columns kept at the right (2 under toggle). The head is "╭" for
 * every tool row: read, write, edit, bash, powershell, grep, find, ls, custom tools (including
 * MCP and codemode), in every state (running, done, failed, folded, expanded).
 */
import { chrome, INDEX, ink, paint, RAIL, TEXT } from "./compact-tools-indices.ts";
import type { Theme } from "@earendil-works/pi-coding-agent";
import type { Component } from "@earendil-works/pi-tui";
import {
	Container,
	sliceByColumn,
	stripTerminalSequences,
	truncateToWidth,
	visibleWidth,
	wrapTextWithAnsi,
} from "@earendil-works/pi-tui";
import { normalizeLineEndings } from "./compact-tools-core.ts";
import type { ToolArgs } from "./compact-tools-types.ts";

export class CachedComponent implements Component {
	/** Most recently used first; at most two widths. */
	private cached: { width: number; lines: string[] }[] = [];

	private readonly renderLines: (width: number) => string[];
	private readonly invalidateSource?: () => void;

	constructor(renderLines: (width: number) => string[], invalidateSource?: () => void) {
		this.renderLines = renderLines;
		this.invalidateSource = invalidateSource;
	}

	render(width: number): string[] {
		const i = this.cached.findIndex((e) => e.width === width);
		if (i >= 0) {
			const [hit] = this.cached.splice(i, 1);
			this.cached.unshift(hit);
			return hit.lines;
		}
		const lines = this.renderLines(width);
		this.cached = [{ width, lines }, ...this.cached].slice(0, 2);
		return lines;
	}

	invalidate(): void {
		this.invalidateSource?.();
		this.cached = [];
	}
}

/**
 * A Container that does not re-copy all child lines on every fullscreen scroll
 * frame. Its lines are kept for a width, and for whatever else `cacheKey()`
 * says they depend on, until a child is added, removed, or invalidated.
 */
export class CachedContainer extends Container {
	/** Most recently used first; at most two widths. */
	private cached: { width: number; key: unknown; lines: string[] }[] = [];

	/** What besides the width the lines depend on, read as each frame is drawn. */
	protected cacheKey(): unknown {
		return undefined;
	}

	private clearRenderCache(): void {
		this.cached = [];
	}

	override addChild(component: Component): void {
		super.addChild(component);
		this.clearRenderCache();
	}

	override removeChild(component: Component): void {
		super.removeChild(component);
		this.clearRenderCache();
	}

	override clear(): void {
		super.clear();
		this.clearRenderCache();
	}

	override render(width: number): string[] {
		const key = this.cacheKey();
		const i = this.cached.findIndex((e) => e.width === width && e.key === key);
		if (i >= 0) {
			const [hit] = this.cached.splice(i, 1);
			this.cached.unshift(hit);
			return hit.lines;
		}
		const lines = super.render(width);
		this.cached = [{ width, key, lines }, ...this.cached].slice(0, 2);
		return lines;
	}

	override invalidate(): void {
		super.invalidate();
		this.clearRenderCache();
	}
}

/**
 * A call line whose status dot is painted when the row is drawn rather than when
 * it was built. The dot follows the row's state and the animation frame without
 * Pi rebuilding the row, and a line is laid out once for each dot it has shown.
 */
export class LiveCallContainer extends CachedContainer {
	private readonly titles = new Map<string, Component>();
	private indicator = "";
	private readonly titleLine: Component = {
		render: (width) => this.title().render(width),
		invalidate: () => this.titles.clear(),
	};

	private readonly paintIndicator: () => string;
	private readonly renderTitle: (indicator: string) => Component;

	/** A closing row the call draws itself: what it shows besides the pill, and the row. */
	private readonly closing?: { key: () => string; row: Component };

	constructor(paintIndicator: () => string, renderTitle: (indicator: string) => Component, closing?: { key: () => string; row: Component }) {
		super();
		this.closing = closing;
		this.paintIndicator = paintIndicator;
		this.renderTitle = renderTitle;
		super.addChild(this.titleLine);
	}

	/** Clearing removes what was added beneath the call line; the line itself stays. */
	override clear(): void {
		super.clear();
		super.addChild(this.titleLine);
	}

	/** The dot, painted now; the title drawn beneath it follows. */
	protected override cacheKey(): string {
		this.indicator = this.paintIndicator();
		return this.closing ? `${this.indicator}|${this.closing.key()}` : this.indicator;
	}

	private title(): Component {
		let title = this.titles.get(this.indicator);
		if (!title) {
			title = this.renderTitle(this.indicator);
			this.titles.set(this.indicator, title);
		}
		return title;
	}
}

export function prefixedText(text: string, firstPrefix: string, continuationPrefix = firstPrefix): Component {
	const prefixWidth = Math.max(visibleWidth(firstPrefix), visibleWidth(continuationPrefix));
	const normalized = text.replace(/\t/g, "   ");
	return new CachedComponent((width) => {
		const lines = wrapTextWithAnsi(normalized, Math.max(1, width - prefixWidth));
		return lines.map((line, index) => `${index === 0 ? firstPrefix : continuationPrefix}${line}`);
	});
}

/**
 * Lines under a tree prefix. With `truncate`, each keeps to one row and ends in an
 * ellipsis, the way Claude Code previews; otherwise each wraps in full. Lines are
 * handled one by one, so the indentation that output starts with survives.
 */
export function prefixedLines(text: string, firstPrefix: string, continuationPrefix: string, truncate: boolean): Component {
	const prefixWidth = Math.max(visibleWidth(firstPrefix), visibleWidth(continuationPrefix));
	const lines = text.replace(/\t/g, "   ").split("\n");
	return new CachedComponent((width) => {
		const available = Math.max(1, width - prefixWidth);
		const rows = truncate
			? lines.map((line) => truncateToWidth(line, available, "…"))
			: lines.flatMap((line) => hardWrapTextWithAnsi(line, available));
		return rows.map((row, index) => `${index === 0 ? firstPrefix : continuationPrefix}${row}`);
	});
}

/**
 * A sentence under a tree prefix, wrapped at words as a terminal UI wraps prose:
 * a word moves to the next row whole, and only a word longer than a row is split.
 */
export function prefixedSentence(text: string, firstPrefix: string, continuationPrefix: string, trimWrappedIndent = false): Component {
	const prefixWidth = Math.max(visibleWidth(firstPrefix), visibleWidth(continuationPrefix));
	const lines = text.replace(/\t/g, "   ").split("\n");
	return new CachedComponent((width) => {
		const available = Math.max(1, width - prefixWidth);
		return lines.flatMap((line) => wrapTextWithAnsi(line, available).map((row, index) => {
			if (trimWrappedIndent && index > 0) {
				const spaces = stripTerminalSequences(row).match(/^ +/u)?.[0].length ?? 0;
				if (spaces) row = sliceByColumn(row, spaces, visibleWidth(row) - spaces, true);
			}
			return truncateToWidth(row, available, "");
		}))
			.map((row, index) => `${index === 0 ? firstPrefix : continuationPrefix}${row}`);
	});
}

/**
 * Output as Claude Code previews it. Each line is wrapped to the width, and rows
 * rather than lines are counted: collapsed, `maxRows` show, all of them when only
 * one more would be hidden, then an optional completion and `more(hidden)`.
 * Claude Code wraps ten columns short of the terminal, so previews break where
 * its own do. Completion rows never count toward the output-preview limit.
 */
export function claudeRows(
	lines: readonly string[],
	firstPrefix: string,
	continuationPrefix: string,
	maxRows: number | undefined,
	more: (hidden: number) => string,
	completion?: Component,
): Component {
	const prefixWidth = Math.max(visibleWidth(firstPrefix), visibleWidth(continuationPrefix));
	return new CachedComponent((width) => {
		const available = Math.max(1, width - prefixWidth);
		const wrapWidth = maxRows === undefined ? available : Math.min(available, Math.max(width - 10, 10));
		let rows = lines.flatMap((line) => hardWrapTextWithAnsi(line, wrapWidth).map((row) => row.trimEnd()));
		const hidden = maxRows !== undefined && rows.length > maxRows + 1 ? rows.length - maxRows : 0;
		if (hidden) rows = rows.slice(0, maxRows);
		const rendered = rows.map((row, index) => `${index === 0 ? firstPrefix : continuationPrefix}${row}`);
		if (completion) rendered.push(...completion.render(width));
		if (hidden) rendered.push((rendered.length ? continuationPrefix : firstPrefix) + truncateToWidth(more(hidden), available, "…"));
		return rendered;
	}, () => completion?.invalidate());
}

export function hardWrapTextWithAnsi(text: string, width: number): string[] {
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

export function renderToolCall(
	title: string,
	details: string | undefined,
	theme: Theme,
	continuationPrefix = chrome(" │ "),
	firstPrefix = " ",
	reserve = 0,
): Component {
	const text = details ? `${title} ${details}` : title;
	const prefixWidth = visibleWidth(continuationPrefix);
	return new CachedComponent((width) => {
		const lines = hardWrapTextWithAnsi(text, width - prefixWidth - reserve);
		return lines.map((line, index) => `${index === 0 ? firstPrefix : continuationPrefix}${line}`);
	});
}

export function styleMultiline(text: string, style: (line: string) => string): string {
	return normalizeLineEndings(text).split("\n").map(style).join("\n");
}

export function renderOutput(output: string, theme: Theme, isError: boolean, rail = chrome(" │ ")): Component | undefined {
	const normalized = normalizeLineEndings(output).trimEnd();
	if (!normalized) return undefined;
	const color = isError ? "error" : "toolOutput";
	const styled = normalized.split("\n").map((line) => ink(color, line)).join("\n");
	return prefixedText(styled, rail);
}

export type CodeDiffLineKind = "context" | "add" | "remove" | "separator";

export type CodeDiffLine = {
	kind: CodeDiffLineKind;
	lineNumber?: number;
	content: string;
	hunk: number;
};

export type CodeDiffOptions = {
	/** Context lines kept around each change; omit to keep every context line the diff carries. */
	contextLines?: number;
	rail?: string;
};

const DIFF_SEPARATOR = "\u22ee";

function parseUnifiedPatch(patch: string, tab: string): CodeDiffLine[] {
	const result: CodeDiffLine[] = [];
	let oldLine = 0;
	let newLine = 0;
	let hunk = -1;
	for (const line of normalizeLineEndings(patch).split("\n")) {
		const header = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/u);
		if (header) {
			if (hunk >= 0) result.push({ kind: "separator", content: DIFF_SEPARATOR, hunk });
			hunk++;
			oldLine = Number(header[1]);
			newLine = Number(header[2]);
			continue;
		}
		if (hunk < 0 || line.startsWith("\\ No newline at end of file")) continue;
		const prefix = line[0];
		const content = line.slice(1).replace(/\t/gu, tab);
		if (prefix === "+") {
			result.push({ kind: "add", lineNumber: newLine++, content, hunk });
		} else if (prefix === "-") {
			result.push({ kind: "remove", lineNumber: oldLine++, content, hunk });
		} else if (prefix === " ") {
			result.push({ kind: "context", lineNumber: newLine++, content, hunk });
			oldLine++;
		}
	}
	return result;
}

function parseDisplayDiff(diff: string, tab: string): CodeDiffLine[] {
	const result: CodeDiffLine[] = [];
	let hunk = 0;
	for (const line of normalizeLineEndings(diff).split("\n")) {
		if (/^\s*\.\.\.\s*$/u.test(line)) {
			result.push({ kind: "separator", content: DIFF_SEPARATOR, hunk: hunk++ });
			continue;
		}
		const match = line.match(/^([ +\-])(\s*\d+)\s(.*)$/u);
		if (!match) continue;
		result.push({
			kind: match[1] === "+" ? "add" : match[1] === "-" ? "remove" : "context",
			lineNumber: Number(match[2]),
			content: match[3]!.replace(/\t/gu, tab),
			hunk,
		});
	}
	return result;
}

/**
 * Parse a standard patch, falling back to Pi's display-oriented numbered diff. A tab
 * becomes `tab`: three columns as Pi draws code, four as Codex does.
 */
export function parseCodeDiff(patch: string, displayDiff = "", tab = "   "): CodeDiffLine[] {
	const parsedPatch = patch ? parseUnifiedPatch(patch, tab) : [];
	return parsedPatch.length > 0 ? parsedPatch : parseDisplayDiff(displayDiff, tab);
}

/** Drop context that sits far from any change so a collapsed preview still reaches the edit. */
export function trimDiffContext(lines: CodeDiffLine[], contextLines: number): CodeDiffLine[] {
	const kept = lines.map((line) => line.kind !== "context");
	lines.forEach((line, index) => {
		if (line.kind !== "add" && line.kind !== "remove") return;
		for (let offset = 1; offset <= contextLines; offset++) {
			if (lines[index - offset]?.kind === "context") kept[index - offset] = true;
			if (lines[index + offset]?.kind === "context") kept[index + offset] = true;
		}
	});
	const result: CodeDiffLine[] = [];
	lines.forEach((line, index) => {
		const previous = result[result.length - 1];
		// One mark per gap: context dropped next to a hunk break the diff already marks
		// would otherwise draw two.
		if (previous?.kind === "separator" && (!kept[index] || line.kind === "separator")) return;
		if (kept[index]) result.push(line);
		else if (previous) result.push({ kind: "separator", content: DIFF_SEPARATOR, hunk: line.hunk });
	});
	while (result.length > 0 && result[result.length - 1]!.kind === "separator") result.pop();
	return result;
}

function hardSliceAnsi(text: string, width: number): string[] {
	const safeWidth = Math.max(1, width);
	const lineWidth = visibleWidth(text);
	if (lineWidth === 0) return [""];
	const slices: string[] = [];
	for (let offset = 0; offset < lineWidth; offset += safeWidth) {
		slices.push(sliceByColumn(text, offset, safeWidth, true));
	}
	return slices;
}

/** The palette index of a row's text: added 2, removed 1, anything else read-only text. */
function codeIndex(kind: CodeDiffLineKind): number {
	return kind === "add" ? INDEX.green : kind === "remove" ? INDEX.red : TEXT;
}

function codeGutter(kind: CodeDiffLineKind, lineNumber: number | undefined, numberWidth: number, signs: boolean): string {
	const number = (lineNumber === undefined ? "" : String(lineNumber)).padStart(numberWidth);
	if (!signs) return paint(RAIL, ` ${number} `);
	const sign = kind === "add" ? "+" : kind === "remove" ? "-" : " ";
	return paint(kind === "context" ? RAIL : codeIndex(kind), ` ${number} ${sign}`);
}

type CodeRowsOptions = {
	/** Show the `+`/`-` sign column and tint changed rows. */
	signs: boolean;
	/** Dim note rendered below the code, such as a read truncation notice. */
	footer?: string;
	/** What each row starts with; the result rail unless a mode draws its own. */
	rail?: string;
};

/** Render numbered code rows shared by edit diffs and read/write file views: text in indices only, no fill. */
function renderCodeRows(lines: CodeDiffLine[], theme: Theme, options: CodeRowsOptions): Component {
	const numberWidth = lines.reduce((maximum, line) => Math.max(maximum, String(line.lineNumber ?? "").length), 1);
	const prefix = options.rail ?? chrome(" \u2502 ");
	const prefixWidth = visibleWidth(prefix);
	const gutterWidth = numberWidth + (options.signs ? 3 : 2);
	return new CachedComponent((width) => {
		const bodyWidth = Math.max(1, width - prefixWidth);
		const codeWidth = Math.max(1, bodyWidth - gutterWidth);
		const output: string[] = [];
		for (const line of lines) {
			if (line.kind === "separator") {
				output.push(prefix + " ".repeat(numberWidth + 1) + ink("toolDiffContext", line.content));
				continue;
			}
			for (const [chunkIndex, chunk] of hardSliceAnsi(line.content, codeWidth).entries()) {
				const gutter = codeGutter(line.kind, chunkIndex === 0 ? line.lineNumber : undefined, numberWidth, options.signs);
				output.push(prefix + gutter + paint(codeIndex(line.kind), chunk));
			}
		}
		if (options.footer) {
			for (const line of wrapTextWithAnsi(options.footer, bodyWidth)) output.push(prefix + ink("dim", line));
		}
		return output;
	});
}

/** Render an edit as a diff with a line-number gutter; added rows are text 2, removed rows text 1. */
export function renderCodeDiff(
	patch: string,
	displayDiff: string,
	path: string,
	theme: Theme,
	options: CodeDiffOptions = {},
): Component | undefined {
	const diff = parseCodeDiff(patch, displayDiff);
	const lines = options.contextLines === undefined ? diff : trimDiffContext(diff, options.contextLines);
	if (lines.length === 0) return undefined;
	return renderCodeRows(lines, theme, { signs: true, rail: options.rail });
}

export type CodeViewOptions = {
	/** File line number of the first code line, e.g. a read's `offset`. */
	startLine?: number;
	footer?: string;
	rail?: string;
};

/** Render file contents with line numbers, matching the edit diff layout. */
export function renderCodeView(code: string, path: string, theme: Theme, options: CodeViewOptions = {}): Component | undefined {
	const normalized = normalizeLineEndings(code).replace(/\n+$/u, "");
	if (!normalized) return undefined;
	const startLine = Math.max(1, Math.floor(options.startLine ?? 1));
	const lines = normalized.split("\n").map((content, index): CodeDiffLine => ({
		kind: "context",
		lineNumber: startLine + index,
		content: content.replace(/\t/gu, "   "),
		hunk: 0,
	}));
	return renderCodeRows(lines, theme, { signs: false, footer: options.footer, rail: options.rail });
}

/**
 * Diff rows the way Codex draws them: `{line} {sign}{code}` after `indent`, the
 * number dim and the sign and text in the change color (no fill). A wrapped row continues under the code; `⋮` marks a gap
 * between hunks. `maxRows` keeps the first rows, as Codex's collapsed preview does.
 */
export function renderCodexDiff(
	lines: readonly CodeDiffLine[],
	path: string,
	theme: Theme,
	indent: string,
	maxRows?: number,
): Component | undefined {
	if (lines.length === 0) return undefined;
	const numberWidth = lines.reduce((maximum, line) => Math.max(maximum, String(line.lineNumber ?? "").length), 1);
	const indentWidth = visibleWidth(indent);
	return new CachedComponent((width) => {
		const codeWidth = Math.max(1, width - indentWidth - numberWidth - 2);
		const output: string[] = [];
		for (const line of lines) {
			if (maxRows !== undefined && output.length >= maxRows) break;
			if (line.kind === "separator") {
				output.push(indent + " ".repeat(numberWidth + 1) + ink("dim", line.content));
				continue;
			}
			const index = codeIndex(line.kind);
			const sign = line.kind === "add" ? paint(index, "+") : line.kind === "remove" ? paint(index, "-") : " ";
			for (const [chunkIndex, chunk] of hardSliceAnsi(line.content, codeWidth).entries()) {
				const gutter = chunkIndex === 0
					? ink("dim", `${String(line.lineNumber ?? "").padStart(numberWidth)} `) + sign
					: " ".repeat(numberWidth + 2);
				// Narrower than the gutter itself, a row is cut rather than drawn past the edge.
				output.push(truncateToWidth(indent + gutter + paint(index, chunk), Math.max(1, width), ""));
			}
		}
		return maxRows === undefined ? output : output.slice(0, maxRows);
	});
}

/**
 * A row that never draws past the pane's edge. Chrome has a width of its own, so in
 * a pane narrower than a prefix the lines are cut to it. Lines are checked again only
 * when the component draws new ones.
 */
export class WithinWidth implements Component {
	private source: string[] | undefined;
	private width: number | undefined;
	private lines: string[] = [];

	private readonly inner: Component;

	constructor(inner: Component) {
		this.inner = inner;
	}

	render(width: number): string[] {
		const lines = this.inner.render(width);
		if (lines !== this.source || width !== this.width) {
			this.source = lines;
			this.width = width;
			this.lines = lines.some((line) => visibleWidth(line) > width)
				? lines.map((line) => visibleWidth(line) > width ? truncateToWidth(line, Math.max(1, width), "") : line)
				: lines;
		}
		return this.lines;
	}

	invalidate(): void {
		this.inner.invalidate?.();
		this.source = undefined;
	}
}

/** Limit a result preview by rendered rows while preserving the full source component for expansion. */
export function limitComponentLines(
	component: Component,
	maximumLines: number,
	theme: Theme,
	footer: (omitted: number) => string = (omitted) => chrome(" │ ")
		+ ink("toolOutput", `… ${omitted} more ${omitted === 1 ? "line" : "lines"}`),
): Component {
	return new CachedComponent((width) => {
		const lines = component.render(width);
		if (lines.length <= maximumLines) return lines;
		return [...lines.slice(0, maximumLines), truncateToWidth(footer(lines.length - maximumLines), Math.max(1, width), "…")];
	}, () => component.invalidate?.());
}

/** Indent another renderer's component under the result rail so it reads as part of the row. */
export function railComponent(component: Component, theme: Theme, rail = chrome(" │ ")): Component {
	const railWidth = visibleWidth(rail);
	return new CachedComponent(
		(width) => component.render(Math.max(1, width - railWidth)).map((line) => rail + line),
		() => component.invalidate?.(),
	);
}

export function renderArguments(args: ToolArgs, theme: Theme, rail = chrome(" │ ")): Component {
	const json = JSON.stringify(args, null, 2) ?? "{}";
	const styled = styleMultiline(json, (line) => ink("toolOutput", line));
	return prefixedText(styled, rail);
}

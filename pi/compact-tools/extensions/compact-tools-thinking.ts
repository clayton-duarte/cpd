/*
 * fold: "toggle", thinking block; see docs/contract.md section 6.9.
 *
 * renderThinkingSections with fold "toggle" (the three views map as follows):
 *   hidden   unchanged: "Thinking…", no toggle row.
 *   summary  = folded. Each section prints its title, then, only when the section has detail,
 *            the row "╰" + toggle.expand + " expand" (chrome rail, same painter as the tool rows).
 *            A section without detail has nothing to expand and no toggle row.
 *   detail   = expanded. Title, the "│" detail lines as today, then for a section with detail
 *            "╰" + toggle.collapse + " collapse · ctrl+t toggle • click to hide" (the hint after
 *            " · ", dim, as today). A section without detail has no toggle row.
 * Every block starts folded (summary); a click toggles it, whatever pi's hideThinkingBlock says.
 * Replaces today's "└ ctrl+t toggle • click to hide" control row; the "└ " prefix is not used.
 */
import { AssistantMessageComponent, type ExtensionAPI, type ExtensionContext, type Theme } from "@earendil-works/pi-coding-agent";
import {
	isKeyRelease,
	isKeyRepeat,
	Key,
	matchesKey,
	stripTerminalSequences,
	visibleWidth,
	wrapTextWithAnsi,
} from "@earendil-works/pi-tui";
import { chrome, fgOpen, ink, paint, TEXT, THINKING_LABEL, thinkingPill } from "./compact-tools-indices.ts";
import { italic } from "../../lib/italic.ts";
import { localTime } from "../../lib/time.ts";
import { hookMethod } from "./compact-tools-hook.ts";
import type { ToggleGlyphs } from "./compact-tools-types.ts";

export type ThinkingView = "summary" | "detail" | "hidden";
export type ThinkingPhase = "summary" | "detail" | "summary-after-detail" | "hidden";

type ThinkingRenderOptions = {
	styleSummary?: (summary: string, sectionIndex: number) => string;
	styleDetailPrefix?: (prefix: string) => string;
	controls?: string | ((hasDetail: boolean) => string | undefined);
	styleControlPrefix?: (prefix: string) => string;
	/** fold: "toggle": the glyphs of the toggle row that replaces the "└ " control row. */
	toggle?: ToggleGlyphs;
	/** `HH:MM` in the process's local time, after the toggle's word (hs-d637); omitted and unwrapped when it does not fit. */
	time?: string;
};

const NEXT_PHASE: Record<ThinkingPhase, ThinkingPhase> = {
	summary: "detail",
	detail: "summary-after-detail",
	"summary-after-detail": "hidden",
	hidden: "summary",
};

export function thinkingViewForPhase(phase: ThinkingPhase): ThinkingView {
	return phase === "summary-after-detail" ? "summary" : phase;
}

export function nextThinkingPhase(phase: ThinkingPhase): ThinkingPhase {
	return NEXT_PHASE[phase];
}

export type ThinkingToggleInput = "toggle" | "repeat" | "release";

export function classifyThinkingToggleInput(data: string): ThinkingToggleInput | undefined {
	if (!matchesKey(data, Key.ctrl("t"))) return undefined;
	if (isKeyRelease(data)) return "release";
	return isKeyRepeat(data) ? "repeat" : "toggle";
}

type ThinkingSection = { summary: string; detail: string };

/** One section: the title is the literal "Thinking…", the whole text is the fold's body. */
function splitThinkingSections(markdown: string): ThinkingSection[] {
	const normalized = markdown.includes("\r") ? markdown.replace(/\r\n?/gu, "\n") : markdown;
	return [{ summary: THINKING_LABEL, detail: normalized.trim() }];
}

export function compactDetail(
	detail: string,
	availableWidth: number,
	stylePrefix: (prefix: string) => string,
): string {
	const output: string[] = [];
	let inFence = false;
	for (const logicalLine of detail.replace(/\t/gu, "   ").split("\n")) {
		const fence = /^\s*(```|~~~)/u.test(logicalLine);
		if (!inFence && fence) {
			// Fences need a real blockquote and blank delimiter to remain valid Markdown.
			output.push("", `> ${logicalLine}`);
			inFence = true;
			continue;
		}
		if (inFence) {
			output.push(logicalLine ? `> ${logicalLine}` : ">");
			if (fence) {
				inFence = false;
				output.push("");
			}
			continue;
		}
		if (!logicalLine) {
			output.push(`${stylePrefix("│")}  `);
			continue;
		}
		const wrapped = wrapTextWithAnsi(logicalLine, Math.max(1, availableWidth - 2));
		for (const line of wrapped) {
			output.push(`${stylePrefix("│ ")}${stripTerminalSequences(line)}  `);
		}
	}
	return output.join("\n");
}

function hardBreak(lines: string[]): string {
	return lines.map((line, index) => (index < lines.length - 1 ? `${line}  ` : line)).join("\n");
}

function sectionsHaveDetail(sections: readonly ThinkingSection[]): boolean {
	return sections.some(({ detail }) => detail.length > 0);
}

export function hasThinkingDetail(markdown: string): boolean {
	return sectionsHaveDetail(splitThinkingSections(markdown));
}

/** `╰` (rail) then `<glyph> <label>` (toggle), then ` · <time>`, then ` · <hint>` when there is a hint. */
export function toggleRow(
	style: (text: string) => string,
	glyph: string,
	label: string,
	hint: string | undefined,
	availableWidth: number,
	time?: string,
): string[] {
	const base = paint(TEXT, ` ${label}`);
	const dated = time === undefined ? base : base + paint(TEXT, ` · ${time}`);
	// A narrow row drops the time rather than wrapping it: the toggle row stays as it was.
	const prefix = time !== undefined && visibleWidth(stripTerminalSequences(dated)) > Math.max(1, availableWidth - 2) ? base : dated;
	const text = prefix + (hint ? style(" · ") + hint : "");
	return wrapTextWithAnsi(text, Math.max(1, availableWidth - 2)).map(
		(line, index) => `${index === 0 ? style("╰") + paint(TEXT, glyph) : "  "}${line}`,
	);
}

function renderThinkingSections(
	sections: readonly ThinkingSection[],
	view: ThinkingView,
	availableWidth: number,
	options: ThinkingRenderOptions = {},
): string {
	if (view === "hidden") return italic(THINKING_LABEL);
	const controls = options.controls ?? ((hasDetail: boolean) =>
		hasDetail ? "ctrl+t toggle • click to hide" : undefined);
	const styleControlPrefix = options.styleControlPrefix ?? ((prefix: string) => prefix);
	return sections
		.map(({ summary, detail }, sectionIndex) => {
			// The summary is never cut short: a provider that sends one unstructured
			// paragraph has no detail to expand into, so the summary is all there is.
			// The host's Markdown component wraps it to the available width.
			const plain = stripTerminalSequences(summary);
			const title = options.styleSummary?.(plain, sectionIndex) ?? plain;
			const sectionControls = view === "detail"
				? typeof controls === "function" ? controls(detail.length > 0) : controls
				: undefined;
			const { toggle } = options;
			// fold: "toggle": summary is folded, detail is expanded; only a section with detail has a toggle.
			const controlLines = toggle
				? detail.length === 0 ? [] : view === "detail"
					? toggleRow(styleControlPrefix, toggle.collapse, "collapse", sectionControls, availableWidth - 2, options.time)
					: toggleRow(styleControlPrefix, toggle.expand, "expand", undefined, availableWidth - 2, options.time)
				: sectionControls
				? wrapTextWithAnsi(sectionControls, Math.max(1, availableWidth - 2)).map(
						(line, index) => `${index === 0 ? styleControlPrefix("└ ") : "  "}${line}`,
					)
				: [];
			if (view === "detail" && detail) {
				const styleDetailPrefix = options.styleDetailPrefix ?? ((prefix: string) => prefix);
				const detailOutput = `${title}  \n${compactDetail(detail, availableWidth - 2, styleDetailPrefix)}`;
				return controlLines.length > 0 ? `${detailOutput}\n${hardBreak(controlLines)}` : detailOutput;
			}
			return hardBreak([title, ...controlLines]);
		})
		.join("\n\n");
}

/** Display-only transformation; the original thinking remains unchanged in session/model context. */
export function renderThinkingView(
	markdown: string,
	view: ThinkingView,
	availableWidth: number,
	options: ThinkingRenderOptions = {},
): string {
	return renderThinkingSections(splitThinkingSections(markdown), view, availableWidth, options);
}

/** The bound controllers, the latest last: pi gives a message no way to name the one that owns it. */
const bound: ThinkingCycleController[] = [];

type ThinkingMessage = {
	hideThinkingBlock: boolean;
	thinkingVisibilityOverrides: Map<number, boolean>;
	contentContainer: { children: Array<{ child?: { defaultTextStyle?: { italic?: boolean }; theme?: { italic: (text: string) => string; code?: (text: string) => string; highlightCode?: (code: string, lang?: string) => string[] }; text?: string; paddingX?: number; setText?: (text: string) => void } }> };
	updateContent(message: unknown, isStreaming?: boolean): void;
};

const upright = (text: string): string => text.replace(/\x1b\[2?3m/gu, "");

const HOOK_NAME = "assistant.thinking";

/**
 * pi's thinking block has cuts we do not want: a click on a visible block records an override that
 * swaps in pi's plain hidden label (never our expanded view), its Markdown is drawn
 * with italic, and its hideThinkingBlock flag decides the first view. All are decided inside
 * AssistantMessageComponent.updateContent, which offers no hook, so it is wrapped once per process
 * (hookMethod: a reload only replaces the behavior, so the newest load's controller owns it): a click
 * override becomes our fold toggle, visibility is ours while a controller is bound, and the italic is
 * cancelled on the body of the thinking children it just built (emphasis, inline code, highlighted code;
 * the agent's reply keeps pi's emphasis); the label "Thinking…" is italic (hs-d557/hs-d587 amended).
 */
function thinkingBehavior(self: ThinkingMessage, args: any[], original: (...args: any[]) => any): unknown {
	const owner = bound[bound.length - 1];
	if (!owner) return original.apply(self, args);
	const clicked = owner.takeClick(self.thinkingVisibilityOverrides);
	owner.noteTimestamp((args[0] as { timestamp?: number } | undefined)?.timestamp);
	const piHidden = self.hideThinkingBlock;
	self.hideThinkingBlock = owner.hidden;
	let result: unknown;
	try {
		result = original.apply(self, args);
	} finally {
		self.hideThinkingBlock = piHidden;
	}
	for (const { child } of self.contentContainer.children) {
		if (!child) continue;
		// Emphasis spans (*em*, _em_, quotes) take their italic from the Markdown's theme: give this thinking
		// child a theme whose italic is the identity (links, lists, tables inherit it; the reply is untouched).
		// pi's Theme.fg hook italicises inline code and highlighted code: strip SGR 3/23 from both so the body stays upright.
		// pi's hidden-label Text keeps its italic: the label is italic.
		if (child.theme) {
			const { code, highlightCode } = child.theme;
			child.theme = {
				...child.theme,
				italic: (text) => text,
				...(code ? { code: (text: string) => upright(code(text)) } : {}),
				...(highlightCode ? { highlightCode: (src: string, lang?: string) => highlightCode(src, lang).map(upright) } : {}),
			};
		}
		if (child.defaultTextStyle?.italic) child.defaultTextStyle = { ...child.defaultTextStyle, italic: false };
		// The block has no horizontal margin: the rail is in column 0 (docs/contract.md 6.9; pi's outputPad is dropped).
		if (owner.folds() && child.paddingX !== undefined) child.paddingX = 0;
	}
	if (clicked) owner.refresh();
	return result;
}

export class ThinkingCycleController {
	private context: ExtensionContext | undefined;
	private phase: ThinkingPhase = "summary";
	private unsubscribe: (() => void) | undefined;
	private theme: Theme | undefined;
	private hasToggleableThinking = false;
	private refreshThinking: (() => void) | undefined;
	/** The Unix-ms timestamp of the assistant message the wrapper last saw, for the toggle row's `HH:MM` (hs-d637). */
	private at: number | undefined;
	/** Fold "toggle" draws the block: its rows take the whole width, with no padding column of pi's own. */
	readonly folds: () => boolean;

	constructor(pi: ExtensionAPI, enabled: () => boolean = () => true, toggle: () => ToggleGlyphs | undefined = () => undefined) {
		this.folds = () => enabled() && toggle() !== undefined;
		const transformer = (markdown: string, context: { messageType: string; availableWidth: number }): string => {
			// Off also applies before session_start, during transcript reconstruction
			// and HTML exports. Do not summarize or restyle native thinking there.
			if (!enabled() || context.messageType !== "assistant-thinking") return markdown;
			const sections = splitThinkingSections(markdown);
			this.hasToggleableThinking ||= sectionsHaveDetail(sections);
			const theme = this.theme;
			const detailTextPrefix = fgOpen(TEXT);
			// Derived once per transform: the rail is drawn for every wrapped line of detail.
			const rail = theme ? chrome : undefined;
			const toggleGlyphs = toggle();
			const time = this.at !== undefined && Number.isFinite(this.at) ? localTime(new Date(this.at).toISOString()) : undefined;
			return renderThinkingSections(sections, thinkingViewForPhase(this.phase), context.availableWidth, {
				// Keep thinking chrome neutral across themes: effort-level and border colors can
				// be intentionally vivid and are not content hierarchy colors.
				styleSummary: theme ? (summary) => (toggleGlyphs ? chrome("╭─") : "") + thinkingPill(summary) : undefined,
				// Prefix styling resets the foreground; resume the host's thinking text color.
				// The rail and connector are the same frame the tool rows draw, so they take
				// the same chrome tone rather than a color name of their own.
				styleDetailPrefix: rail && detailTextPrefix
					? (prefix) => rail(prefix) + detailTextPrefix
					: undefined,
				// With the toggle the collapse row has no hint; the fold: status └ row keeps it.
				controls: theme && !toggleGlyphs
					? (hasDetail) => hasDetail ? ink("dim", "ctrl+t toggle • click to hide") : undefined
					: () => undefined,
				styleControlPrefix: rail,
				toggle: toggleGlyphs,
				time,
			});
		};
		pi.registerMarkdownTransformer(transformer);
	}

	/**
	 * A click on a visible block: pi recorded "hide it" (override true). Drop it and flip our own fold
	 * instead, so the block expands or folds rather than turning into pi's plain label.
	 */
	takeClick(overrides: Map<number, boolean>): boolean {
		if (overrides.size === 0) return false;
		overrides.clear();
		if (!this.hasToggleableThinking) return false;
		this.phase = this.phase !== "hidden" && thinkingViewForPhase(this.phase) === "detail" ? "summary" : "detail";
		return true;
	}

	/** The updateContent wrapper hands the message's timestamp here; the toggle row reads the latest (hs-d637). */
	noteTimestamp(timestamp: number | undefined): void {
		this.at = timestamp;
	}

	get hidden(): boolean {
		return this.phase === "hidden";
	}

	refresh(): void {
		this.refreshThinking?.();
	}

	bind(ctx: ExtensionContext): void {
		this.unsubscribe?.();
		if (!bound.includes(this)) bound.push(this);
		this.context = ctx;
		this.theme = ctx.ui.theme;
		this.hasToggleableThinking = false;
		// Every block starts folded whatever pi's hideThinkingBlock says. Pi wraps
		// the hidden label in thinkingText, so an inner text span keeps it consistent with
		// completed summary titles.
		this.phase = "summary";
		const hiddenLabel = ink("text", this.theme.bold(italic(THINKING_LABEL)));
		// Pi does not currently expose transcript invalidation directly. Updating
		// this label rebuilds assistant Markdown and requests a render without
		// changing the message or its hidden/visible state.
		this.refreshThinking = () => ctx.ui.setHiddenThinkingLabel(hiddenLabel);
		const prototype = AssistantMessageComponent.prototype as unknown as { updateContent: ((...args: any[]) => any) & { original?: unknown } };
		// An older load's once-per-process wrapper (own `original`, no hook record) is unwrapped first.
		if (!Object.hasOwn(prototype, Symbol.for(`pi-compact-tools.hook.${HOOK_NAME}`))
			&& typeof prototype.updateContent === "function"
			&& typeof prototype.updateContent.original === "function") {
			prototype.updateContent = prototype.updateContent.original as typeof prototype.updateContent;
		}
		hookMethod(prototype, "updateContent", HOOK_NAME, thinkingBehavior as never);
		this.refreshThinking();
		this.unsubscribe = ctx.ui.onTerminalInput((data) => {
			const input = classifyThinkingToggleInput(data);
			if (input === undefined) return undefined;
			// Kitty-capable macOS terminals report held keys as repeat events. Treating
			// those as presses can skip detail before the user sees it.
			if (input === "release" || input === "repeat") return { consume: true };
			// With no provider-supplied detail, leave Ctrl+T to Pi's normal
			// visible/hidden toggle instead of inserting an identical detail phase.
			if (!this.hasToggleableThinking) return undefined;
			this.phase = nextThinkingPhase(this.phase);
			if (this.phase === "detail" || this.phase === "summary-after-detail") {
				// Visible-to-visible transitions are owned by this extension. Rebuild the
				// Markdown and prevent Pi's built-in two-state toggle from hiding it.
				this.refreshThinking?.();
				return { consume: true };
			}
			// summary-after-detail -> hidden and hidden -> summary intentionally reach
			// Pi's existing two-state handler. This keeps visibility/settings in sync
			// and clears built-in per-block click overrides.
			return undefined;
		});
	}

	dispose(restoreNative = false): void {
		const context = this.context;
		const unsubscribe = this.unsubscribe;
		this.context = undefined;
		this.unsubscribe = undefined;
		this.refreshThinking = undefined;
		if (bound.includes(this)) bound.splice(bound.indexOf(this), 1);
		this.hasToggleableThinking = false;
		this.theme = undefined;
		try { unsubscribe?.(); }
		finally { if (restoreNative) context?.ui.setHiddenThinkingLabel(); }
	}
}

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
 * Replaces today's "└ ctrl+t toggle • click to hide" control row; the "└ " prefix is not used.
 */
import { AssistantMessageComponent, getAgentDir, type ExtensionAPI, type ExtensionContext, type Theme } from "@earendil-works/pi-coding-agent";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
	isKeyRelease,
	isKeyRepeat,
	Key,
	matchesKey,
	stripTerminalSequences,
	wrapTextWithAnsi,
} from "@earendil-works/pi-tui";
import { chrome, fgOpen, ink, paint, TEXT, thinkingPill } from "./compact-tools-indices.ts";
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
	return [{ summary: "Thinking…", detail: normalized.trim() }];
}

function compactDetail(
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

/** `╰` (rail) then `<glyph> <label>` (toggle), then ` · <hint>` when there is a hint. */
function toggleRow(
	style: (text: string) => string,
	glyph: string,
	label: string,
	hint: string | undefined,
	availableWidth: number,
): string[] {
	const text = paint(TEXT, ` ${label}`) + (hint ? style(" · ") + hint : "");
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
	if (view === "hidden") return "Thinking…";
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
					? toggleRow(styleControlPrefix, toggle.collapse, "collapse", sectionControls, availableWidth)
					: toggleRow(styleControlPrefix, toggle.expand, "expand", undefined, availableWidth)
				: sectionControls
				? wrapTextWithAnsi(sectionControls, Math.max(1, availableWidth - 2)).map(
						(line, index) => `${index === 0 ? styleControlPrefix("└ ") : "  "}${line}`,
					)
				: [];
			if (view === "detail" && detail) {
				const styleDetailPrefix = options.styleDetailPrefix ?? ((prefix: string) => prefix);
				const connector = `${styleDetailPrefix("│")}  `;
				const detailOutput = `${title}  \n${connector}\n${compactDetail(detail, availableWidth, styleDetailPrefix)}`;
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

function getInitialThinkingPhase(): ThinkingPhase {
	try {
		const settings = JSON.parse(readFileSync(join(getAgentDir(), "settings.json"), "utf8")) as {
			hideThinkingBlock?: unknown;
		};
		return settings.hideThinkingBlock === true ? "hidden" : "summary";
	} catch {
		return "summary";
	}
}

/** The bound controllers, the latest last: pi gives a message no way to name the one that owns it. */
const bound: ThinkingCycleController[] = [];

type ThinkingMessage = {
	thinkingVisibilityOverrides: Map<number, boolean>;
	contentContainer: { children: Array<{ child?: { defaultTextStyle?: { italic?: boolean }; text?: string; setText?: (text: string) => void } }> };
	updateContent(message: unknown, isStreaming?: boolean): void;
};

type PatchedUpdate = ((this: ThinkingMessage, message: unknown, isStreaming?: boolean) => void) & { original?: unknown };

/**
 * pi's thinking block has two cuts we do not want: a click on a visible block records an override that
 * swaps in pi's plain hidden label (never our expanded view), and its Markdown/hidden label are drawn
 * with italic. Both are decided inside AssistantMessageComponent.updateContent, which offers no hook,
 * so it is wrapped once while a controller is bound: a click override becomes our fold toggle, and the
 * italic is cancelled on the thinking children it just built (the agent's reply keeps pi's emphasis).
 */
function patchedUpdate(original: (message: unknown, isStreaming?: boolean) => void): PatchedUpdate {
	const wrapper: PatchedUpdate = function (this: ThinkingMessage, message, isStreaming) {
		const owner = bound[bound.length - 1];
		const clicked = owner?.takeClick(this.thinkingVisibilityOverrides) ?? false;
		original.call(this, message, isStreaming);
		if (!owner) return;
		for (const { child } of this.contentContainer.children) {
			if (!child) continue;
			if (child.defaultTextStyle?.italic) child.defaultTextStyle = { ...child.defaultTextStyle, italic: false };
			else if (typeof child.text === "string" && child.setText) child.setText(child.text.replace(/\x1b\[2?3m/gu, ""));
		}
		if (clicked) owner.refresh();
	};
	wrapper.original = original;
	return wrapper;
}

export class ThinkingCycleController {
	private context: ExtensionContext | undefined;
	private phase: ThinkingPhase = "summary";
	private unsubscribe: (() => void) | undefined;
	private theme: Theme | undefined;
	private hasToggleableThinking = false;
	private refreshThinking: (() => void) | undefined;

	constructor(pi: ExtensionAPI, enabled: () => boolean = () => true, toggle: () => ToggleGlyphs | undefined = () => undefined) {
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
			});
		};
		pi.registerMarkdownTransformer(transformer);
	}

	/**
	 * A click on a visible block: pi recorded "hide it" (override true). Drop it and flip our own fold
	 * instead, so the block expands or folds rather than turning into pi's plain label.
	 */
	takeClick(overrides: Map<number, boolean>): boolean {
		if (overrides.size === 0 || !this.hasToggleableThinking || this.phase === "hidden") return false;
		if (![...overrides.values()].includes(true)) return false;
		overrides.clear();
		this.phase = thinkingViewForPhase(this.phase) === "detail" ? "summary" : "detail";
		return true;
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
		// Match Pi's persisted host visibility after startup or /reload. When the
		// host is hidden, Markdown transformers are not invoked at all. Pi wraps
		// this label in thinkingText, so an inner text span keeps it consistent with
		// completed summary titles.
		this.phase = getInitialThinkingPhase();
		const hiddenLabel = ink("text", this.theme.bold("Thinking…"));
		// Pi does not currently expose transcript invalidation directly. Updating
		// this label rebuilds assistant Markdown and requests a render without
		// changing the message or its hidden/visible state.
		this.refreshThinking = () => ctx.ui.setHiddenThinkingLabel(hiddenLabel);
		const prototype = AssistantMessageComponent.prototype as unknown as { updateContent: PatchedUpdate };
		if (!prototype.updateContent.original) prototype.updateContent = patchedUpdate(prototype.updateContent);
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

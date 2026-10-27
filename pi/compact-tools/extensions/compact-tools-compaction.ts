/*
 * The compaction fold (docs/contract.md section 6.9, hs-d650): pi-core's compaction notice
 * (CompactionSummaryMessageComponent, mounted for a `compactionSummary` entry) is drawn as a cpd
 * fold in the compact style with `fold: toggle` only.
 *
 * pi's `updateDisplay` is replaced, not decorated (the class offers no hook): the body clears the
 * Box and mounts one MouseRegion over the fold rows, so a left click and pi's Ctrl+O (which calls
 * `setExpanded` on every chat child) reach the same state. With no owner, `fold: status`,
 * `style: off`, claude or codex, the hook runs pi's original display untouched.
 */
import {
	CompactionSummaryMessageComponent,
	type ExtensionAPI,
	type ExtensionContext,
	type Theme,
} from "@earendil-works/pi-coding-agent";
import { MouseRegion, type Component } from "@earendil-works/pi-tui";
import { chrome, fgOpen, TEXT, thinkingPill } from "./compact-tools-indices.ts";
import { hookMethod } from "./compact-tools-hook.ts";
import { compactDetail, toggleRow } from "./compact-tools-thinking.ts";
import type { ToggleGlyphs } from "./compact-tools-types.ts";

/** The bound controllers, the latest last: pi gives a message no way to name the one that owns it. */
const bound: CompactionFoldController[] = [];
const HOOK_NAME = "compaction.updateDisplay";

/** The horizontal padding pi gave each notice, kept while the fold drops it (the fold has no horizontal margin) and put back when pi's own row returns. */
const piPadding = new WeakMap<object, number>();

/** The fold rows, as the contract fixes them: a `╭─` head with the compaction pill, the toggle row, and the summary on rails when open. The rows take the whole width: the rail is in column 0 and the text ends at column `width - 3`. */
function foldRows(self: { message: { tokensBefore: number; summary: string }; expanded: boolean }, width: number, toggle: ToggleGlyphs): string[] {
	const message = self.message;
	const metadata = chrome(`Compacted from ${message.tokensBefore.toLocaleString()} tokens`);
	const closing = self.expanded
		? toggleRow(chrome, toggle.collapse, "collapse", metadata, width - 2)
		: toggleRow(chrome, toggle.expand, "expand", metadata, width - 2);
	let rows = chrome("╭─") + thinkingPill("compaction");
	if (self.expanded) {
		// The rail and the text take the Thinking detail renderer's painters: rail 8, text 7.
		rows += "\n" + compactDetail(message.summary, width - 2, (prefix) => chrome(prefix) + fgOpen(TEXT));
	}
	return (rows + "\n" + closing.join("\n")).split("\n");
}

/** The replacement body: pi's display while no controller wants the fold, cpd's rows while one does. */
function compactionBehavior(self: any, args: any[], original: (...args: any[]) => any): unknown {
	const owner = bound[bound.length - 1];
	const toggle = owner?.toggle();
	if (!owner || !owner.enabled() || toggle === undefined) {
		const padding = piPadding.get(self);
		if (padding !== undefined) {
			self.paddingX = padding;
			piPadding.delete(self);
		}
		return original.apply(self, args);
	}
	if (!piPadding.has(self)) piPadding.set(self, self.paddingX);
	self.paddingX = 0;
	self.clear();
	self.addChild(new MouseRegion({
		render: (width: number): string[] => foldRows(self, width, toggle),
		invalidate() {},
	} satisfies Component, (event) => {
		if (event.type !== "click" || event.button !== "left") return undefined;
		self.setExpanded(!self.expanded);
		return { handled: true };
	}));
	return undefined;
}

export class CompactionFoldController {
	readonly enabled: () => boolean;
	readonly toggle: () => ToggleGlyphs | undefined;
	private theme: Theme | undefined;

	constructor(_pi: ExtensionAPI, enabled: () => boolean, toggle: () => ToggleGlyphs | undefined) {
		this.enabled = enabled;
		this.toggle = toggle;
	}

	bind(ctx: ExtensionContext): void {
		this.theme = ctx.ui.theme;
		if (!bound.includes(this)) bound.push(this);
		hookMethod(CompactionSummaryMessageComponent.prototype, "updateDisplay", HOOK_NAME, compactionBehavior);
	}

	dispose(): void {
		this.theme = undefined;
		if (bound.includes(this)) bound.splice(bound.indexOf(this), 1);
	}
}

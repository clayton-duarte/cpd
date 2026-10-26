import { Theme } from "@earendil-works/pi-coding-agent";
import { italic } from "../../lib/italic.ts";
import { hookMethod } from "./compact-tools-hook.ts";

/** Contract 6.1.1 (pro-d38): under the dango theme these Theme.fg tokens are drawn in SGR 3. */
export const ITALIC_THEME = "dango";
export const ITALIC_TOKENS = ["mdCode", "syntaxFunction", "syntaxType", "customMessageLabel"] as const;

export function italicFg(self: { name: string }, args: readonly unknown[], original: (...args: any[]) => string): string {
	const out = original.apply(self, args as unknown[]);
	return self.name === ITALIC_THEME && (ITALIC_TOKENS as readonly unknown[]).includes(args[0]) ? italic(out) : out;
}

export function hookItalicSpans(enabled: () => boolean, target: object = Theme.prototype): void {
	hookMethod(target, "fg", "theme.fg.italic", (self: any, args: any, original: any) =>
		enabled() ? italicFg(self, args, original) : original.apply(self, args));
}

import { Theme } from "@earendil-works/pi-coding-agent";

/** Stub: the italic spans of contract 6.1.1 (pro-d38) are not implemented yet. */
export const ITALIC_THEME = "dango";
export const ITALIC_TOKENS = ["mdCode", "syntaxFunction", "syntaxType", "customMessageLabel"] as const;

export function italicFg(self: { name: string }, args: readonly unknown[], original: (...args: any[]) => string): string {
	return original.apply(self, args as unknown[]);
}

export function hookItalicSpans(_enabled: () => boolean, _target: object = Theme.prototype): void {}

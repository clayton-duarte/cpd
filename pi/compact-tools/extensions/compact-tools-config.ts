/*
 * STUB, red-test period of the compact-tools extension (docs/contract.md section 6.9). The shapes are the fork's
 * `compact-tools-config.ts` and `compact-tools-types.ts`; the values of DEFAULT_CONFIG are upstream's, not yet cpd's
 * (fold "toggle", the cockpit's glyphs, seven tools). The implementation replaces this file with the copy.
 */
export type DisplayStyle = "compact" | "claude" | "codex" | "off";

export interface CompactToolsConfig {
	mode: "normal" | "silent";
	style: DisplayStyle;
	tools: string[];
	previewLines: number;
	fold: "status" | "toggle";
	toggle: { expand: string; collapse: string };
	auto_compact: Record<string, boolean>;
	custom_tools: { enabled: boolean; auto_compact: boolean; exclude: string[] };
}

export const DEFAULT_CONFIG: CompactToolsConfig = {
	mode: "normal",
	style: "compact",
	tools: ["read", "write", "edit", "bash"],
	previewLines: 10,
	fold: "status",
	toggle: { expand: "\u25B8", collapse: "\u25BE" },
	auto_compact: { read: true, write: true, edit: false, bash: true, powershell: true, grep: true, find: true, ls: true },
	custom_tools: { enabled: true, auto_compact: true, exclude: [] },
};

export function mergeConfig(_base: CompactToolsConfig, _value: unknown, _path: string): CompactToolsConfig {
	throw new Error("compact-tools extension: not implemented (docs/contract.md section 6.9)");
}

export function loadConfig(_cwd?: string, _projectTrusted = false): CompactToolsConfig {
	throw new Error("compact-tools extension: not implemented (docs/contract.md section 6.9)");
}

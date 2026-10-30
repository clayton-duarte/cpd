/*
 * A double load (docs/contract.md section 6.9, "A double load"). While the settings still list another copy of this
 * extension (the npm package pi-compact-tools, or a git source of that name), pi would load both and both would register the
 * same tool renderers, command and hooks. The extension stands down instead: `otherCopy` is the detection, a pure function
 * of the `packages` lists of the two settings files, and `standDown` registers the one `session_start` handler that says so.
 */
import { join } from "node:path";
import { CONFIG_DIR_NAME, getAgentDir } from "@earendil-works/pi-coding-agent";
import type { ExtensionAPI, PackageSource } from "@earendil-works/pi-coding-agent";
import { readOnlySettings } from "../../lib/settings-read.ts";

const PACKAGE = "pi-compact-tools";
/** Survives /reload, which evaluates the factory anew in the same process: the line is said once per process. */
const SAID = Symbol.for("cpd.compact-tools.double-load-said");

export interface OtherCopy {
	source: string;
	scope: "global" | "project";
}

export interface PackageLists {
	global: readonly PackageSource[];
	project: readonly PackageSource[];
}

const sourceOf = (entry: PackageSource): string => (typeof entry === "string" ? entry : entry.source);

/** The package identity pi compares: an npm spec's name without its version, a git source's repository name without ref or `.git`. */
function isThisPackage(source: string): boolean {
	if (source.startsWith("npm:")) {
		const spec = source.slice("npm:".length);
		// A scoped name starts with "@": its version, if any, follows the second "@".
		const at = spec.indexOf("@", 1);
		return (at === -1 ? spec : spec.slice(0, at)) === PACKAGE;
	}
	if (/^(git:|git\+|git@|https?:\/\/|ssh:\/\/)/u.test(source)) {
		const last = source.split(/[/:]/u).at(-1) ?? "";
		const at = last.indexOf("@");
		return (at === -1 ? last : last.slice(0, at)).replace(/\.git$/u, "") === PACKAGE;
	}
	return false;
}

/** The first entry of the global list, then of the project list, that is another copy of this extension. */
export function otherCopy(packages: PackageLists): OtherCopy | undefined {
	for (const scope of ["global", "project"] as const) {
		for (const entry of packages[scope]) {
			const source = sourceOf(entry);
			if (isThisPackage(source)) return { source, scope };
		}
	}
	return undefined;
}

/** The other copy the pi settings of the agent directory and of `cwd` list, if any. */
export function findOtherCopy(cwd: string): { copy: OtherCopy; file: string } | undefined {
	const settings = readOnlySettings(cwd, getAgentDir());
	const copy = otherCopy({
		global: settings.getGlobalSettings().packages ?? [],
		project: settings.getProjectSettings().packages ?? [],
	});
	if (!copy) return undefined;
	const file = copy.scope === "global" ? join(getAgentDir(), "settings.json") : join(cwd, CONFIG_DIR_NAME, "settings.json");
	return { copy, file };
}

/** Registers nothing but one `session_start` handler that says, once per process, that this copy stands down. */
export function standDown(pi: ExtensionAPI, found: { copy: OtherCopy; file: string }): void {
	const line = `cpd compact-tools: ${found.copy.source} is also loaded (${found.file}): cpd's own copy stands down and registers nothing. Remove it from packages in ${found.file} and run /reload.`;
	pi.on("session_start", (_event, ctx) => {
		const slot = globalThis as Record<symbol, unknown>;
		if (slot[SAID]) return;
		slot[SAID] = true;
		if (ctx.mode === "tui") ctx.ui.notify(line, "warning");
		else process.stderr.write(`${line}\n`);
	});
}

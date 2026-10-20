// @ts-nocheck: the extension it imports is not copied yet; implementing.md "The compact-tools extension" point 4 removes this line, and layout.test.ts fails until it is gone
import { SUPPORTED_TOOLS } from "../../compact-tools/extensions/compact-tools-types.ts";
import type { RowResolver } from "../../compact-tools/extensions/compact-tools-custom.ts";

/** Test-only UI lookup. These objects are never registered as agent tools. */
export function uiRenderers() {
	const resolve = (globalThis as unknown as Record<symbol, RowResolver>)[Symbol.for("pi-compact-tools.custom.resolver.v2")];
	return SUPPORTED_TOOLS.flatMap((name) => {
		const renderers = resolve?.({ toolName: name });
		return renderers ? [{ name, ...renderers }] : [];
	});
}

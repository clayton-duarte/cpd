/*
 * The compaction fold (docs/contract.md section 6.9, hs-d650): pi-core's compaction notice
 * (CompactionSummaryMessageComponent, mounted for a `compactionSummary` entry) is drawn as a cpd
 * fold in the compact style with `fold: toggle` only.
 *
 * STUB: this installs no fold, so the red tests in
 * pi/test/compact-tools/compaction-fold.test.ts fail for the rendering they pin. The implementation
 * Job replaces this file.
 */
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import type { ToggleGlyphs } from "./compact-tools-types.ts";

export class CompactionFoldController {
	constructor(_pi: ExtensionAPI, _enabled: () => boolean, _toggle: () => ToggleGlyphs | undefined) {}
	bind(_ctx: ExtensionContext): void {}
	dispose(): void {}
}

/*
 * STUB, red-test period of the compact-tools extension (docs/contract.md section 6.9). It exists so the tests
 * under pi/test/compact-tools/ resolve the entry point; the implementation replaces this file with the copy of
 * the fork's compact-tools.ts (docs/implementing.md, "The compact-tools extension", point 1).
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function compactTools(_pi: ExtensionAPI): void {
	throw new Error("compact-tools extension: not implemented (docs/contract.md section 6.9)");
}

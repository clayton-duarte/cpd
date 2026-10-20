/*
 * STUB, red-test period of the compact-tools extension (docs/contract.md section 6.9, "A double load"). The
 * implementation replaces this file with the module docs/implementing.md, "The compact-tools extension",
 * point 2, specifies.
 */
import type { PackageSource } from "@earendil-works/pi-coding-agent";

export interface OtherCopy {
	source: string;
	scope: "global" | "project";
}

export function otherCopy(_packages: { global: readonly PackageSource[]; project: readonly PackageSource[] }): OtherCopy | undefined {
	throw new Error("compact-tools extension: not implemented (docs/contract.md section 6.9)");
}

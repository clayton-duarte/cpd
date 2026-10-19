import { test } from "node:test";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import type { PiApi } from "../lib/pi.ts";

/** The adapter is written against `PiApi`; pi hands it an `ExtensionAPI`. `tsc` (npm run typecheck) fails here when pi's API drifts from what the adapter uses (pi-ci-G1). */
export const asAdapterSees = (real: ExtensionAPI): PiApi => real;

test("pi's ExtensionAPI is a PiApi (checked by tsc)", () => {});

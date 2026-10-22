/**
 * Golden silent-mode activity line: every frame of one breath, in palette indices
 * only, the same whichever theme Pi runs. Record it with UPDATE_GOLDEN=1, only when
 * a change to the output is intended.
 */
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { renderActivityDots } from "../../compact-tools/extensions/compact-tools-activity.ts";
import { readable } from "./indicator-harness.ts";

const BREATH_FRAMES = 26;
const goldenPath = join(import.meta.dirname, "golden", "activity.json");
const draw = () => Array.from({ length: BREATH_FRAMES }, (_, frame) => readable(renderActivityDots(frame, 40)[0]!));

test("activity golden: every frame of a breath", () => {
	if (process.env.UPDATE_GOLDEN === "1") writeFileSync(goldenPath, `${JSON.stringify(draw(), null, "\t")}\n`);
	assert.ok(existsSync(goldenPath), "no golden activity");
	assert.deepEqual(draw(), JSON.parse(readFileSync(goldenPath, "utf8")));
});

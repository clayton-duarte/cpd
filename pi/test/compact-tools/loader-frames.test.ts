/** The pi loader table (docs/contract.md 6.9): eight moon phases, one every 100 ms, in colour 6. */
import assert from "node:assert/strict";
import test from "node:test";
import { LOADER_FRAMES, LOADER_INTERVAL_MS, loaderFrame } from "../../compact-tools/extensions/compact-tools-loader.ts";
import { MOON_FRAMES } from "./loader-frames.ts";

test("the loader frames are the eight moon phases, new moon to waning crescent", () => {
	assert.deepEqual([...LOADER_FRAMES], [...MOON_FRAMES]);
});

test("no frame repeats and the last leads into the first", () => {
	assert.equal(new Set(LOADER_FRAMES).size, LOADER_FRAMES.length);
	assert.equal(LOADER_FRAMES.length, 8);
	assert.equal(LOADER_FRAMES.at(-1), "\u{F0F65}", "waning crescent last");
	assert.equal(LOADER_FRAMES[0], "\u{F0F64}", "new moon first");
	const plain = (frame: string) => frame.replace(/\x1b\[[0-9;]*m/gu, "");
	assert.equal(plain(loaderFrame(7 * 100)), "\u{F0F65}");
	assert.equal(plain(loaderFrame(8 * 100)), "\u{F0F64}", "after the last frame comes the first");
});

test("the loader still ticks every 100 ms in colour 6", () => {
	assert.equal(LOADER_INTERVAL_MS, 100);
	for (let step = 0; step < 16; step++) {
		const frame = loaderFrame(step * 100);
		assert.ok(frame.startsWith("\x1b[36m"), JSON.stringify(frame));
		assert.equal(frame.replace(/\x1b\[[0-9;]*m/gu, ""), MOON_FRAMES[step % 8], `step ${step}`);
		assert.equal(loaderFrame(step * 100 + 99), frame, "no step inside 100 ms");
	}
});

/** The pi loader table (docs/contract.md 6.9): 28 moon_alt phases, one every 50 ms, in colour 6. */
import assert from "node:assert/strict";
import test from "node:test";
import { LOADER_FRAMES, LOADER_INTERVAL_MS, loaderFrame } from "../../compact-tools/extensions/compact-tools-loader.ts";
import { MOON_FRAMES } from "./loader-frames.ts";

test("the loader frames are the 28 moon_alt phases in order", () => {
	assert.deepEqual([...LOADER_FRAMES], [...MOON_FRAMES]);
});

test("no frame repeats and the last leads into the first", () => {
	assert.equal(new Set(LOADER_FRAMES).size, LOADER_FRAMES.length);
	assert.equal(LOADER_FRAMES.length, 28);
	assert.equal(LOADER_FRAMES.at(-1), String.fromCodePoint(0xe3e2), "waning crescent 6 last");
	assert.equal(LOADER_FRAMES[0], String.fromCodePoint(0xe3e3), "new moon first");
	const plain = (frame: string) => frame.replace(/\x1b\[[0-9;]*m/gu, "");
	assert.equal(plain(loaderFrame(27 * 50)), String.fromCodePoint(0xe3e2));
	assert.equal(plain(loaderFrame(28 * 50)), String.fromCodePoint(0xe3e3), "after the last frame comes the first");
});

test("the loader ticks every 50 ms in colour 6", () => {
	assert.equal(LOADER_INTERVAL_MS, 50);
	for (let step = 0; step < 57; step++) {
		const frame = loaderFrame(step * 50);
		assert.ok(frame.startsWith("\x1b[36m"), JSON.stringify(frame));
		assert.equal(frame.replace(/\x1b\[[0-9;]*m/gu, ""), MOON_FRAMES[step % 28], `step ${step}`);
		assert.equal(loaderFrame(step * 50 + 49), frame, "no step inside 50 ms");
	}
});

/** The wrapSegments content cache: same content, same rows, and no re-segmenting. */
import "./pinned-themes.ts";
import assert from "node:assert/strict";
import test, { afterEach, mock } from "node:test";
import { WRAP_CACHE_CHARS, wrapSegments, type Segment } from "../../compact-tools/extensions/compact-tools-codex.ts";

afterEach(() => mock.restoreAll());

/** Counts every Intl.Segmenter segment call until the mocks are restored. */
function spy(): { count: () => number } {
	let count = 0;
	const original = Intl.Segmenter.prototype.segment;
	mock.method(Intl.Segmenter.prototype, "segment", function (this: Intl.Segmenter, ...args: Parameters<Intl.Segmenter["segment"]>) {
		count++;
		return original.apply(this, args);
	});
	return { count: () => count };
}

const plain = (rows: readonly (readonly Readonly<Segment>[])[]) => JSON.parse(JSON.stringify(rows));

test("wrap cache returns the identical rows for an unchanged array", () => {
	const segments: Segment[] = [{ text: "same-array alpha beta gamma delta" }];
	assert.equal(wrapSegments(segments, 10, 8), wrapSegments(segments, 10, 8));
});

test("wrap cache returns the identical rows for a fresh array with the same content", () => {
	const make = (): Segment[] => [{ text: "fresh-array alpha ", color: "dim" }, { text: "beta gamma delta" }];
	assert.equal(wrapSegments(make(), 10, 8), wrapSegments(make(), 10, 8));
});

test("wrap cache wraps again after a segment text is mutated", () => {
	const segments: Segment[] = [{ text: "mutate-text alpha beta" }];
	const probe = spy();
	const first = plain(wrapSegments(segments, 8, 8));
	assert.ok(probe.count() > 0);
	const before = probe.count();
	segments[0]!.text = "mutate-text gamma delta";
	const second = wrapSegments(segments, 8, 8);
	assert.ok(probe.count() > before, "segmented again");
	assert.notDeepEqual(plain(second), first);
	assert.ok(second.flat().some((piece) => piece.text.includes("gamma")));
});

test("wrap cache wraps again after a segment color changes", () => {
	const segments: Segment[] = [{ text: "mutate-color alpha beta", color: "dim" }];
	const probe = spy();
	const first = plain(wrapSegments(segments, 40, 40));
	const before = probe.count();
	assert.ok(before > 0);
	segments[0]!.color = "accent";
	const second = wrapSegments(segments, 40, 40);
	assert.ok(probe.count() > before, "segmented again");
	assert.equal(second[0]![0]!.color, "accent");
	assert.equal(first[0][0].color, "dim");
});

test("wrap cache wraps again after the first width or the rest width changes", () => {
	const segments: Segment[] = [{ text: "widths alpha beta gamma delta epsilon zeta" }];
	const probe = spy();
	const base = plain(wrapSegments(segments, 12, 10));
	const afterBase = probe.count();
	assert.ok(afterBase > 0);
	const firstChanged = wrapSegments(segments, 20, 10);
	assert.ok(probe.count() > afterBase, "first width miss");
	assert.notDeepEqual(plain(firstChanged), base);
	const afterFirst = probe.count();
	const restChanged = wrapSegments(segments, 12, 20);
	assert.ok(probe.count() > afterFirst, "rest width miss");
	assert.notDeepEqual(plain(restChanged), base);
});

test("wrap cache gives back the earlier rows when the old text is restored", () => {
	const segments: Segment[] = [{ text: "restore alpha beta gamma" }];
	const first = plain(wrapSegments(segments, 9, 9));
	segments[0]!.text = "restore other words entirely";
	assert.notDeepEqual(plain(wrapSegments(segments, 9, 9)), first);
	segments[0]!.text = "restore alpha beta gamma";
	assert.deepEqual(plain(wrapSegments(segments, 9, 9)), first);
});

test("wrap cache keeps a joined emoji whole when it serves a repeat", () => {
	const family = "👨‍👩‍👧‍👦";
	const make = (): Segment[] => [{ text: `emoji-whole a${family}b` }];
	for (const _ of [0, 1]) {
		const pieces = wrapSegments(make(), 3, 3).flat().map((piece) => piece.text);
		assert.ok(pieces.includes(family) || pieces.some((text) => text.includes(family)), "family emoji stays in one piece");
		assert.ok(pieces.every((text) => !text.includes("\u200d") || text.includes(family)));
	}
});

test("wrap cache does not segment again for an identical repeat", () => {
	const probe = spy();
	wrapSegments([{ text: "repeat-count alpha beta gamma" }], 10, 8);
	const after = probe.count();
	assert.ok(after > 0);
	wrapSegments([{ text: "repeat-count alpha beta gamma" }], 10, 8);
	assert.equal(probe.count(), after);
});

test("wrap cache never shares rows between different calls", () => {
	const pairs: Array<[Segment[], Segment[]]> = [
		[[{ text: "share a\u0002\u0001b" }], [{ text: "share a" }, { text: "b" }]],
		[[{ text: "x", color: "dim" }], [{ text: "x" }]],
		[[{ text: "a\u0000b" }], [{ text: "a" }, { text: "b" }]],
		[[{ text: "a,b" }], [{ text: "a" }, { text: "b" }]],
		[[{ text: 'q"a' }], [{ text: "q" }, { text: "a" }]],
		[[{ text: "ab" }, { text: "c" }], [{ text: "a" }, { text: "bc" }]],
	];
	for (const [left, right] of pairs) {
		const one = wrapSegments(left, 20, 20);
		const two = wrapSegments(right, 20, 20);
		assert.notEqual(one, two);
		assert.deepEqual(plain(wrapSegments(left, 20, 20)), plain(one));
		assert.deepEqual(plain(wrapSegments(right, 20, 20)), plain(two));
	}
	assert.equal(wrapSegments([{ text: "x", color: "dim" }], 20, 20)[0]![0]!.color, "dim");
	assert.equal(wrapSegments([{ text: "x" }], 20, 20)[0]![0]!.color, undefined);
});

test("wrap cache serves frozen rows", () => {
	const rows = wrapSegments([{ text: "frozen alpha beta gamma", color: "dim" }], 8, 8);
	assert.ok(Object.isFrozen(rows), "rows");
	for (const row of rows) {
		assert.ok(Object.isFrozen(row), "row");
		for (const piece of row) assert.ok(Object.isFrozen(piece), "segment");
	}
	assert.throws(() => { (rows[0]![0] as { text: string }).text = "changed"; }, TypeError);
});

test("wrap cache drops the least recently used rows past its character limit", () => {
	const size = Math.floor(WRAP_CACHE_CHARS * 0.4);
	const make = (word: string): Segment[] => [{ text: `${word} `.repeat(Math.ceil(size / (word.length + 1))) }];
	const probe = spy();
	const a = "limita";
	const b = "limitb";
	const c = "limitc";
	wrapSegments(make(a), 40, 40);
	wrapSegments(make(b), 40, 40);
	wrapSegments(make(a), 40, 40);
	wrapSegments(make(c), 40, 40);
	let before = probe.count();
	wrapSegments(make(a), 40, 40);
	assert.equal(probe.count(), before, "A hits");
	wrapSegments(make(c), 40, 40);
	assert.equal(probe.count(), before, "C hits");
	wrapSegments(make(b), 40, 40);
	assert.ok(probe.count() > before, "B misses");
});

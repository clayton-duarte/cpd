import { mock } from "node:test";

/** Counts every Intl.Segmenter segment call until the mocks are restored. */
export function spy(): { count: () => number } {
	let count = 0;
	const original = Intl.Segmenter.prototype.segment;
	mock.method(Intl.Segmenter.prototype, "segment", function (this: Intl.Segmenter, ...args: Parameters<Intl.Segmenter["segment"]>) {
		count++;
		return original.apply(this, args);
	});
	return { count: () => count };
}

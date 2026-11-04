import { expect, it } from "vitest";
import { findAnchor, isNearBottom, layoutRows, scrollTopForAnchor, visibleRange } from "./virtual.ts";

const keys = (n: number, p = "k"): string[] => Array.from({ length: n }, (_, i) => `${p}${i}`);

it("layoutRows sums measured heights and falls back to the estimate", () => {
  const empty = layoutRows([], new Map());
  expect(empty.offsets).toEqual([0]);
  expect(empty.total).toBe(0);
  const l = layoutRows(["a", "b", "c"], new Map([["b", 30]]), 10);
  expect(l.keys).toEqual(["a", "b", "c"]);
  expect(l.offsets).toEqual([0, 10, 40, 50]);
  expect(l.total).toBe(50);
  expect(layoutRows(["a"], new Map()).total).toBe(96);
});

it("visibleRange returns only rows intersecting the viewport plus overscan", () => {
  const l = layoutRows(keys(10), new Map(), 100); // row i = [100i, 100i+100)
  expect(visibleRange(l, 500, 200, 0)).toEqual({ start: 5, end: 7 });
  expect(visibleRange(l, 500, 200, 100)).toEqual({ start: 4, end: 8 });
  // overscan edge touching a row boundary: [400, 800] includes row 4 start..row 8 start
  expect(visibleRange(l, 500, 200, 100).start).toBe(4);
  expect(visibleRange(layoutRows([], new Map()), 0, 800)).toEqual({ start: 0, end: 0 });
});

it("visibleRange of 1000 estimated rows mounts at most 60 rows", () => {
  const l = layoutRows(keys(1000), new Map());
  for (const viewport of [800, 1600]) {
    for (const top of [0, 12345, l.total - viewport]) {
      const r = visibleRange(l, top, viewport);
      expect(r.end - r.start).toBeGreaterThan(0);
      expect(r.end - r.start).toBeLessThanOrEqual(60);
    }
  }
});

it("visibleRange clamps when scrollTop is past the end or negative", () => {
  const l = layoutRows(keys(10), new Map(), 100);
  expect(visibleRange(l, -500, 200, 0)).toEqual({ start: 0, end: 2 });
  const past = visibleRange(l, 99999, 200, 0);
  expect(past.end).toBe(10);
  expect(past.start).toBeLessThanOrEqual(9);
  expect(past.start).toBeGreaterThanOrEqual(0);
});

it("findAnchor returns the row at the top of the viewport with its inner offset", () => {
  const l = layoutRows(["a", "b", "c"], new Map([["b", 30]]), 10);
  expect(findAnchor(l, 25)).toEqual({ key: "b", intra: 15 });
  expect(findAnchor(l, 10)).toEqual({ key: "b", intra: 0 });
  expect(findAnchor(l, 0)).toEqual({ key: "a", intra: 0 });
  expect(findAnchor(layoutRows([], new Map()), 0)).toBeNull();
});

it("scrollTopForAnchor restores the same visual position after rows are prepended", () => {
  const before = layoutRows(["a", "b", "c"], new Map([["b", 30]]), 10);
  const anchor = findAnchor(before, 25);
  expect(anchor).not.toBeNull();
  const after = layoutRows([...keys(40, "new"), "a", "b", "c"], new Map([["b", 30]]), 10);
  const top = scrollTopForAnchor(after, anchor!);
  expect(top).toBe(400 + 25);
  expect(scrollTopForAnchor(after, { key: "missing", intra: 3 })).toBeNull();
});

it("isNearBottom is true within the threshold and false beyond it", () => {
  expect(isNearBottom({ scrollTop: 952, clientHeight: 800, scrollHeight: 1800 }, 48)).toBe(true); // distance exactly 48
  expect(isNearBottom({ scrollTop: 951, clientHeight: 800, scrollHeight: 1800 }, 48)).toBe(false);
  expect(isNearBottom({ scrollTop: 1000, clientHeight: 800, scrollHeight: 1800 }, 48)).toBe(true);
  expect(isNearBottom({ scrollTop: 0, clientHeight: 800, scrollHeight: 800 })).toBe(true);
});

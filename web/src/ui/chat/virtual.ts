import type { Disposable } from "../../protocol/runtime.ts";

export const ESTIMATED_ROW_PX = 96;
export const OVERSCAN_PX = 400;
export const PIN_THRESHOLD_PX = 48;
export const DEFAULT_VIEWPORT_PX = 800;

export interface Layout {
  keys: readonly string[];
  offsets: readonly number[]; // length keys.length + 1, offsets[0] = 0
  total: number;
}
export function layoutRows(keys: readonly string[], heights: ReadonlyMap<string, number>, estimate = ESTIMATED_ROW_PX): Layout {
  const offsets = new Array<number>(keys.length + 1);
  offsets[0] = 0;
  for (let i = 0; i < keys.length; i++) offsets[i + 1] = offsets[i]! + (heights.get(keys[i]!) ?? estimate);
  return { keys, offsets, total: offsets[keys.length]! };
}
// first index in [0, n] whose predicate is true (predicate monotone false->true)
function firstWhere(n: number, pred: (i: number) => boolean): number {
  let lo = 0;
  let hi = n;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (pred(mid)) hi = mid;
    else lo = mid + 1;
  }
  return lo;
}
export function visibleRange(layout: Layout, scrollTop: number, viewport: number, overscanPx = OVERSCAN_PX): { start: number; end: number } {
  const n = layout.keys.length;
  if (n === 0) return { start: 0, end: 0 };
  const top = Math.min(Math.max(scrollTop, 0), layout.total);
  const lo = top - overscanPx;
  const hi = top + viewport + overscanPx;
  const start = Math.min(firstWhere(n, (i) => layout.offsets[i + 1]! > lo), n - 1);
  const end = firstWhere(n, (i) => layout.offsets[i]! >= hi);
  return { start, end: Math.max(start, end) };
}
export interface Anchor {
  key: string;
  intra: number;
}
export function findAnchor(layout: Layout, scrollTop: number): Anchor | null {
  const n = layout.keys.length;
  if (n === 0) return null;
  const top = Math.min(Math.max(scrollTop, 0), layout.total);
  const i = Math.min(firstWhere(n, (j) => layout.offsets[j + 1]! > top), n - 1);
  return { key: layout.keys[i]!, intra: top - layout.offsets[i]! };
}
export function scrollTopForAnchor(layout: Layout, anchor: Anchor): number | null {
  const i = layout.keys.indexOf(anchor.key);
  return i < 0 ? null : layout.offsets[i]! + anchor.intra;
}
export interface Metrics {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}
export function isNearBottom(m: Metrics, threshold = PIN_THRESHOLD_PX): boolean {
  return m.scrollHeight - m.scrollTop - m.clientHeight <= threshold;
}
export interface SizeObserver {
  observe(el: Element, onHeight: (height: number) => void): Disposable;
}
export function browserSizes(): SizeObserver {
  if (typeof ResizeObserver === "undefined") return { observe: () => ({ dispose() {} }) };
  return {
    observe(el, onHeight) {
      const ro = new ResizeObserver((entries) => {
        for (const entry of entries) onHeight(entry.borderBoxSize?.[0]?.blockSize ?? el.getBoundingClientRect().height);
      });
      ro.observe(el);
      let done = false;
      return {
        dispose() {
          if (done) return;
          done = true;
          ro.unobserve(el);
          ro.disconnect();
        },
      };
    },
  };
}

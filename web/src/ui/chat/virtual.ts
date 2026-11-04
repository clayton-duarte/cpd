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
export function layoutRows(_keys: readonly string[], _heights: ReadonlyMap<string, number>, _estimate = ESTIMATED_ROW_PX): Layout {
  throw new Error("not implemented");
}
export function visibleRange(_layout: Layout, _scrollTop: number, _viewport: number, _overscanPx = OVERSCAN_PX): { start: number; end: number } {
  throw new Error("not implemented");
}
export interface Anchor {
  key: string;
  intra: number;
}
export function findAnchor(_layout: Layout, _scrollTop: number): Anchor | null {
  throw new Error("not implemented");
}
export function scrollTopForAnchor(_layout: Layout, _anchor: Anchor): number | null {
  throw new Error("not implemented");
}
export interface Metrics {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}
export function isNearBottom(_m: Metrics, _threshold = PIN_THRESHOLD_PX): boolean {
  throw new Error("not implemented");
}
export interface SizeObserver {
  observe(el: Element, onHeight: (height: number) => void): Disposable;
}
export function browserSizes(): SizeObserver {
  throw new Error("not implemented");
}

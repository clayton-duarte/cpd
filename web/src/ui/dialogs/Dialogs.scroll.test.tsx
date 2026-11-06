// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { LegendDialog } from "./LegendDialog.tsx";
import { LogsDialog } from "./LogsDialog.tsx";
import type { LogEvent } from "./LogsDialog.tsx";

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(() => vi.unstubAllGlobals());

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });
const show = (el: React.ReactElement) => { mounted = mount(el); return mounted; };
const ev = (id: number, text: string): LogEvent => ({ id, at: "2026-01-02T03:04:05.000Z", kind: "info", text });

it("the logs dialog rows scroll with a ScrollArea", () => {
  show(<LogsDialog open onClose={vi.fn()} events={[ev(1, "a"), ev(2, "b")]} />);
  const rows = document.body.querySelector('[data-part="log-rows"]');
  expect(rows).not.toBeNull();
  expect(rows?.closest('[data-slot="scroll-area"]')).not.toBeNull();
  expect(rows?.className).not.toContain("overflow-y-auto");
});

it("the legend dialog sections scroll with a ScrollArea", () => {
  show(<LegendDialog open onClose={vi.fn()} />);
  const dialog = document.body.querySelector('[data-part="legend-dialog"]');
  expect(dialog).not.toBeNull();
  const area = dialog?.querySelector("h3")?.closest('[data-slot="scroll-area"]');
  expect(area).not.toBeNull();
  const h3s = Array.from(dialog?.querySelectorAll("h3") ?? []);
  expect(h3s.length).toBeGreaterThan(0);
  expect(h3s.every((h) => area?.contains(h) === true)).toBe(true);
  const h2 = dialog?.querySelector("h2");
  expect(h2).not.toBeNull();
  expect(area?.contains(h2 ?? null)).toBe(false);
  expect(dialog?.className).toContain("overflow-hidden");
});

// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { DocsDialog } from "./DocsDialog.tsx";

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  Element.prototype.scrollIntoView = vi.fn();
  Element.prototype.hasPointerCapture = vi.fn(() => false);
});
afterEach(() => vi.unstubAllGlobals());

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });
const body = document.body;
const show = (onClose: () => void = vi.fn()) => { mounted = mount(<DocsDialog open onClose={onClose} />); };
const q = (sel: string): Element => {
  const el = body.querySelector(sel);
  if (el === null) throw new Error(`not found: ${sel}`);
  return el;
};
const openArtifacts = () => {
  const trigger = q('[data-tab-trigger="artifacts"]');
  act(() => { trigger.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 })); });
};
const text = (sel: string) => q(sel).textContent ?? "";

it("docs dialog opens on the flowchart tab with the flow canvas", () => {
  show();
  const dialog = q('[data-part="docs-dialog"]');
  expect(q('[data-tab-trigger="flowchart"]').textContent).toBe("Issue Flowchart");
  expect(q('[data-tab-trigger="artifacts"]').textContent).toBe("Documents & Lifecycles");
  expect(q('[data-tab-trigger="flowchart"]').getAttribute("role")).toBe("tab");
  expect(q('[data-tab-trigger="artifacts"]').getAttribute("role")).toBe("tab");
  expect(dialog.querySelector('[data-tab="flowchart"] [data-part="flow-canvas"]')).not.toBeNull();
  const scroll = dialog.querySelector('[data-slot="scroll-area"]');
  expect(scroll).not.toBeNull();
  expect(scroll?.querySelector('[data-tab="flowchart"]')).not.toBeNull();
});

it("docs dialog shows the 7 documents D1 to D7 on the artifacts tab", () => {
  show();
  openArtifacts();
  const heads = ["D1 · Plan", "D2 · Context Records", "D3 · Plan Brief", "D4 · Explore Report",
    "D5 · Workflows + Plan Outline", "D6 · Job Specs", "D7 · Lead Branch Commits & Pull Request"];
  heads.forEach((h, i) => expect(text(`[data-artifact="D${i + 1}"]`)).toContain(h));
  expect(text('[data-artifact="D1"]')).toContain("shaping → done");
  expect(text('[data-artifact="D4"]')).toContain("explore/<job>.md");
  expect(text('[data-artifact="D5"]')).toContain("done_when");
  expect(q('[data-part="docs-dialog"] [data-slot="scroll-area"] [data-tab="artifacts"]')).not.toBeNull();
});

it("docs dialog documents the three agent lifecycles", () => {
  show();
  openArtifacts();
  const manager = text('[data-lifecycle="manager"]');
  for (const s of ["Steps 01–05", "Step 20", "dormant", "06–19"]) expect(manager).toContain(s);
  const lead = text('[data-lifecycle="lead"]');
  for (const s of ["Architect", "Air-Traffic Controller", "06–13", "14–20"]) expect(lead).toContain(s);
  expect(text('[data-lifecycle="worker"]')).toContain("1 Worker = 1 Job = 1 Commit");
});

it("docs dialog documents the four shaping seams", () => {
  show();
  openArtifacts();
  for (const n of [1, 2, 3, 4]) expect(text(`[data-seam="${n}"]`)).toContain("planned, not implemented");
  expect(text('[data-seam="1"]')).toContain("lead_session");
  expect(text('[data-seam="3"]')).toContain("escalation");
  expect(text('[data-seam="4"]')).toContain("grounding");
});

it("docs dialog calls onClose when dismissed", () => {
  const onClose = vi.fn();
  show(onClose);
  act(() => { body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  expect(onClose).toHaveBeenCalledTimes(1);
});

// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { SettingsDialog } from "./SettingsDialog.tsx";
import type { SettingsRead } from "./SettingsDialog.tsx";

// Radix (external) needs these in jsdom: inert doubles.
beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  Element.prototype.scrollIntoView = vi.fn();
  Element.prototype.hasPointerCapture = vi.fn(() => false);
});
afterEach(() => vi.unstubAllGlobals());

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });
const body = document.body;

const read = {
  tiers: { decide: "JX", auxiliar: "GX", worker: "H1", lead: "S1", manager: "O2" },
  experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true },
  concurrency: 3,
} as unknown as SettingsRead;

const ROLES = ["decide", "auxiliar", "worker", "lead", "manager"] as const;
const select = (role: string, part: "model" | "effort"): HTMLSelectElement | null =>
  body.querySelector(`select[aria-label="${role} ${part}"]`);
const pick = (role: string, part: "model" | "effort", value: string): void => {
  const el = select(role, part);
  if (el === null) throw new Error(`the dialog must show a ${role} ${part} selector`);
  act(() => { el.value = value; el.dispatchEvent(new Event("change", { bubbles: true })); });
};
const saveButton = (): HTMLButtonElement | undefined =>
  Array.from(body.querySelectorAll("button")).find((b) => b.textContent === "Save") as HTMLButtonElement | undefined;
const clickSave = (): void => { act(() => { saveButton()?.dispatchEvent(new MouseEvent("click", { bubbles: true })); }); };

// The ladder (core tier.rs): letter rank J<G<H<S<O, then effort 0<X<1<2.
const LETTERS = "JGHSO";
const EFFORTS = "0X12";
const rank = (role: string): number => {
  const m = select(role, "model")?.value ?? "?";
  const e = select(role, "effort")?.value ?? "?";
  return LETTERS.indexOf(m) * 10 + EFFORTS.indexOf(e);
};

it("options and ladder derive from catalog without local constants", () => {
  // Test that SettingsDialog derives model and effort options from the catalog passed via settings/props,
  // without relying on local ladder constants.
  const catalogRead = {
    tiers: { decide: "JX", auxiliar: "GX", worker: "H3", lead: "O2", manager: "O4" },
    catalog: {
      tiers: [
        { tier: "JX", letter: "J", effort: "X", model: "decide-model" },
        { tier: "GX", letter: "G", effort: "X", model: "gemini" },
        { tier: "H2", letter: "H", effort: "2", model: "haiku" },
        { tier: "H3", letter: "H", effort: "3", model: "haiku" },
        { tier: "S3", letter: "S", effort: "3", model: "sonnet" },
        { tier: "S4", letter: "S", effort: "4", model: "sonnet" },
        { tier: "O2", letter: "O", effort: "2", model: "opus" },
        { tier: "O4", letter: "O", effort: "4", model: "opus" },
      ],
      ranges: {
        decide: { floor: "JX", ceiling: "JX", allowed: ["JX"] },
        auxiliar: { floor: "GX", ceiling: "GX", allowed: ["GX"] },
        worker: { floor: "H2", ceiling: "S3", allowed: ["H2", "H3", "S3"] },
        lead: { floor: "S4", ceiling: "O2", allowed: ["S4", "O2"] },
        manager: { floor: "O2", ceiling: "O4", allowed: ["O2", "O4"] },
      },
    },
    experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true },
    concurrency: 3,
  } as unknown as SettingsRead;

  mounted = mount(<SettingsDialog open read={catalogRead} onSave={vi.fn()} onCancel={vi.fn()} />);

  // Model options should only contain models from the catalog, not the old hardcoded list with M, K, L, P, A
  const workerModelOptions = Array.from(select("worker", "model")?.options ?? []).map((o) => o.value);
  expect(workerModelOptions).not.toEqual(expect.arrayContaining(["M", "K", "L", "P", "A"]));
  expect(workerModelOptions).toEqual(expect.arrayContaining(["H", "S"]));

  // Worker effort options should include 3 (from H3), which was missing in old EFFORTS
  const workerEffortOptions = Array.from(select("worker", "effort")?.options ?? []).map((o) => o.value);
  expect(workerEffortOptions).toContain("3");

  // Worker effort 3 should be selected currently
  expect(select("worker", "effort")?.value).toBe("3");
});


it("moving the lead boundary below the worker ceiling moves the worker ceiling coherently", () => {
  mounted = mount(<SettingsDialog open read={read} onSave={vi.fn()} onCancel={vi.fn()} />);
  expect(rank("worker")).toBeLessThan(rank("lead"));
  // Lead drops from S1 to G1, under the worker's H1: the worker ceiling follows it down.
  pick("lead", "model", "G");
  expect(select("lead", "model")?.value).toBe("G");
  expect(select("lead", "effort")?.value).toBe("1");
  expect(rank("worker")).toBeLessThan(rank("lead"));
  expect(rank("lead")).toBeLessThan(rank("manager"));
  expect(saveButton()?.disabled).toBe(false);
});

it("saves per-role model and effort", () => {
  const onSave = vi.fn();
  mounted = mount(<SettingsDialog open read={read} onSave={onSave} onCancel={vi.fn()} />);
  pick("auxiliar", "model", "G");
  pick("auxiliar", "effort", "1");
  clickSave();
  expect(onSave).toHaveBeenCalledExactlyOnceWith({ tiers: { auxiliar: { model: "G", effort: "1" } } });
});

it("an incoherent set cannot be saved", () => {
  // The read itself breaks the strict ascent worker < lead: the dialog refuses to save it.
  const incoherent = { ...read, tiers: { ...read.tiers, worker: "O1", lead: "S1" } } as unknown as SettingsRead;
  const onSave = vi.fn();
  mounted = mount(<SettingsDialog open read={incoherent} onSave={onSave} onCancel={vi.fn()} />);
  expect(select("worker", "model")).not.toBeNull();
  expect(saveButton()?.disabled).toBe(true);
  clickSave();
  expect(onSave).not.toHaveBeenCalled();
});

it("saving only a changed concurrency sends it", () => {
  const onSave = vi.fn();
  mounted = mount(<SettingsDialog open read={read} onSave={onSave} onCancel={vi.fn()} />);
  expect(select("worker", "model")).not.toBeNull();
  const input = body.querySelector('[aria-label="concurrency"]') as HTMLInputElement;
  act(() => { input.value = "12"; input.dispatchEvent(new Event("input", { bubbles: true })); });
  clickSave();
  expect(onSave).toHaveBeenCalledExactlyOnceWith({ concurrency: 12 });
});

it("a changed decide tier is included in the save", () => {
  const onSave = vi.fn();
  mounted = mount(<SettingsDialog open read={read} onSave={onSave} onCancel={vi.fn()} />);
  pick("decide", "effort", "0");
  clickSave();
  expect(onSave).toHaveBeenCalledExactlyOnceWith({ tiers: { decide: { model: "J", effort: "0" } } });
});

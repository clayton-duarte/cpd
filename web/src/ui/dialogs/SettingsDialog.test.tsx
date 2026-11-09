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

it("each role shows a model selector and a separate effort selector", () => {
  mounted = mount(<SettingsDialog open read={read} onSave={vi.fn()} onCancel={vi.fn()} />);
  const want: Record<string, [string, string]> = {
    decide: ["J", "X"], auxiliar: ["G", "X"], worker: ["H", "1"], lead: ["S", "1"], manager: ["O", "2"],
  };
  for (const role of ROLES) {
    expect(select(role, "model")?.value).toBe(want[role]![0]);
    expect(select(role, "effort")?.value).toBe(want[role]![1]);
  }
  const letters = Array.from(select("worker", "model")?.options ?? []).map((o) => o.value);
  expect(letters).toEqual(expect.arrayContaining(["H", "S", "O", "G", "M", "K", "L", "P", "A", "J"]));
  const efforts = Array.from(select("worker", "effort")?.options ?? []).map((o) => o.value);
  expect(efforts.sort()).toEqual(["0", "1", "2", "X"]);
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

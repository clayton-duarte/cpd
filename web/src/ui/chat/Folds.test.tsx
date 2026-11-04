// @vitest-environment jsdom
import { expect, it } from "vitest";
import type { FoldItem, FoldKind } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { FoldRow } from "./Folds.tsx";

const fold = (foldKind: FoldKind, defaultOpen: boolean): FoldItem => ({ kind: "fold", id: `f:${foldKind}`, foldKind, title: `Title ${foldKind}`, body: "body **text**", defaultOpen });

function open(item: FoldItem): { isOpen: boolean; title: string | undefined; kind: string | null; body: string } {
  const m = mount(<FoldRow item={item} />);
  const d = m.container.querySelector('details[data-kind="fold"]') as HTMLDetailsElement;
  const r = { isOpen: d.open, title: d.querySelector("[data-part=marker-text]")?.textContent ?? undefined, kind: d.getAttribute("data-fold"), body: d.textContent ?? "" };
  expect(d.querySelector("strong")?.textContent).toBe("text");
  m.unmount();
  return r;
}

it("cpd-context fold is closed by default", () => {
  const r = open(fold("cpd-context", false));
  expect(r.isOpen).toBe(false);
  expect(r.kind).toBe("cpd-context");
  expect(r.title).toBe("Title cpd-context");
});

it("cpd-plan-summary fold is open by default", () => {
  const r = open(fold("cpd-plan-summary", true));
  expect(r.isOpen).toBe(true);
  expect(r.kind).toBe("cpd-plan-summary");
  expect(r.body).toContain("body");
});

it("cpd-plan-decision and cpd-skills folds are closed by default", () => {
  expect(open(fold("cpd-plan-decision", false)).isOpen).toBe(false);
  expect(open(fold("cpd-skills", false)).isOpen).toBe(false);
});

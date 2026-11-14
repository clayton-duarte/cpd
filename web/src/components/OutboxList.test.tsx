// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { OutboxEntry } from "../state/outbox.ts";
import { mount } from "../testing/render.tsx";
import { OutboxList } from "./OutboxList.tsx";

let view: { unmount(): void } | null = null;
afterEach(() => { view?.unmount(); view = null; });
const entry = (id: string, state: OutboxEntry["state"], error?: string): OutboxEntry => ({
  id, kind: "action", payload: {}, state, createdAt: "2026-01-01T00:00:00Z", ...(error === undefined ? {} : { error }),
});

it("renders nothing for an empty outbox", () => {
  const v = (view = mount(<OutboxList entries={[]} onRetry={() => {}} onDiscard={() => {}} />));
  expect(v.container.querySelector('[data-part="outbox-list"]')).toBeNull();
});
it("shows each entry's state, and Retry and Discard only on failed ones", () => {
  const v = (view = mount(<OutboxList entries={[entry("a", "queued"), entry("b", "failed", "boom")]} onRetry={() => {}} onDiscard={() => {}} />));
  const rows = [...v.container.querySelectorAll('[data-part="outbox-entry"]')];
  expect(rows.map((r) => r.getAttribute("data-state"))).toEqual(["queued", "failed"]);
  expect(rows[0]?.querySelector("button")).toBeNull();
  expect(rows[1]?.querySelector('[data-part="outbox-error"]')?.textContent).toBe("boom");
  expect([...(rows[1]?.querySelectorAll("button") ?? [])].map((b) => b.textContent)).toEqual(["Retry", "Discard"]);
});
it("Retry and Discard report the entry id", () => {
  const onRetry = vi.fn();
  const onDiscard = vi.fn();
  const v = (view = mount(<OutboxList entries={[entry("b", "failed", "boom")]} onRetry={onRetry} onDiscard={onDiscard} />));
  const [retry, discard] = [...v.container.querySelectorAll("button")];
  act(() => { retry?.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
  act(() => { discard?.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
  expect(onRetry).toHaveBeenCalledWith("b");
  expect(onDiscard).toHaveBeenCalledWith("b");
});

// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { CrashReport } from "../electron/health.ts";
import { mount } from "../testing/render.tsx";
import { ErrorBoundary } from "./ErrorBoundary.tsx";

let mounted: { unmount(): void } | null = null;
beforeEach(() => { vi.spyOn(console, "error").mockImplementation(() => {}); });
afterEach(() => { mounted?.unmount(); mounted = null; vi.restoreAllMocks(); });

function Bomb({ armed }: { armed: { on: boolean } }) {
  if (armed.on) throw new Error("kaboom");
  return <p data-ok>fine</p>;
}

it("boundary renders its children when nothing throws", () => {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  const armed = { on: false };
  act(() => root.render(<ErrorBoundary name="Pane"><Bomb armed={armed} /></ErrorBoundary>));
  mounted = { unmount() { act(() => root.unmount()); container.remove(); } };
  expect(container.querySelector("[data-ok]")?.textContent).toBe("fine");
  expect(container.querySelector('[role="alert"]')).toBeNull();
  // and it stays a working boundary: a later throw is caught, not propagated
  armed.on = true;
  act(() => root.render(<ErrorBoundary name="Pane"><Bomb armed={{ on: true }} /></ErrorBoundary>));
  expect(container.querySelector('[role="alert"]')).not.toBeNull();
});

it("boundary renders the fallback with the pane name when a child throws", () => {
  const view = mount(<ErrorBoundary name="Pane"><Bomb armed={{ on: true }} /></ErrorBoundary>);
  mounted = view;
  const alert = view.container.querySelector('[role="alert"]');
  expect(alert).not.toBeNull();
  expect(alert!.textContent).toContain("Pane");
  expect(alert!.textContent).toContain("kaboom");
  expect(view.container.querySelector("[data-ok]")).toBeNull();
});

it("boundary reports the crash once with source error-boundary", () => {
  const onCrash = vi.fn<(r: CrashReport) => void>();
  mounted = mount(<ErrorBoundary name="Pane" onCrash={onCrash}><Bomb armed={{ on: true }} /></ErrorBoundary>);
  expect(onCrash).toHaveBeenCalledTimes(1);
  expect(onCrash.mock.calls[0]![0]).toMatchObject({ source: "error-boundary", error: "kaboom" });
});

it("boundary retry button renders the children again", () => {
  const armed = { on: true };
  const view = mount(<ErrorBoundary name="Pane"><Bomb armed={armed} /></ErrorBoundary>);
  mounted = view;
  armed.on = false;
  const retry = [...view.container.querySelectorAll("button")].find((b) => b.textContent === "Retry");
  expect(retry).toBeDefined();
  act(() => { retry!.click(); });
  expect(view.container.querySelector("[data-ok]")?.textContent).toBe("fine");
  expect(view.container.querySelector('[role="alert"]')).toBeNull();
});

it("boundary of a pane leaves its siblings mounted", () => {
  const view = mount(
    <div>
      <ErrorBoundary name="Left"><Bomb armed={{ on: true }} /></ErrorBoundary>
      <ErrorBoundary name="Right"><p data-sibling>alive</p></ErrorBoundary>
    </div>,
  );
  mounted = view;
  expect(view.container.querySelector("[data-sibling]")?.textContent).toBe("alive");
  expect(view.container.querySelector('[role="alert"]')?.textContent).toContain("Left");
});

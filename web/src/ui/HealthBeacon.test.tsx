// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { HealthBeacon } from "./HealthBeacon.tsx";
import type { HealthBeaconProps } from "./HealthBeacon.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const mounted: { unmount(): void }[] = [];
afterEach(() => { for (const view of mounted.splice(0)) view.unmount(); });

function rootWith(children: number): HTMLElement {
  const el = document.createElement("div");
  for (let i = 0; i < children; i++) el.appendChild(document.createElement("section"));
  return el;
}

/** Renders the beacon into a detached container; rerender delivers a later render (the snapshot). */
function render(props: HealthBeaconProps) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => root.render(<HealthBeacon {...props} />));
  const view = {
    rerender(next: HealthBeaconProps) { act(() => root.render(<HealthBeacon {...next} />)); },
    unmount() { act(() => root.unmount()); container.remove(); },
  };
  mounted.push(view);
  return view;
}

it("beacon reports healthy once after the first snapshot with a non-empty root and no errors", () => {
  const onHealthy = vi.fn();
  const el = rootWith(1);
  // The cockpit is empty: no snapshot has arrived, so a mount reports nothing.
  const view = render({ live: false, root: () => el, errors: () => 0, onHealthy });
  expect(onHealthy).not.toHaveBeenCalled();
  // The first snapshot arrives (the store goes live) and the beacon reports exactly once.
  view.rerender({ live: true, root: () => el, errors: () => 0, onHealthy });
  expect(onHealthy).toHaveBeenCalledTimes(1);
});

it("beacon stays silent when an error was counted", () => {
  const onHealthy = vi.fn();
  const el = rootWith(1);
  render({ live: true, root: () => el, errors: () => 1, onHealthy });
  expect(onHealthy).not.toHaveBeenCalled();
  const control = vi.fn();
  render({ live: true, root: () => el, errors: () => 0, onHealthy: control });
  expect(control).toHaveBeenCalledTimes(1); // the same beacon does report when clean
});

it("beacon stays silent when the root is empty", () => {
  const onHealthy = vi.fn();
  render({ live: true, root: () => rootWith(0), errors: () => 0, onHealthy });
  expect(onHealthy).not.toHaveBeenCalled();
  const control = vi.fn();
  render({ live: true, root: () => rootWith(1), errors: () => 0, onHealthy: control });
  expect(control).toHaveBeenCalledTimes(1); // the same beacon does report with children
  const missing = vi.fn();
  render({ live: true, root: () => null, errors: () => 0, onHealthy: missing });
  expect(missing).not.toHaveBeenCalled();
});

it("beacon reports once across rerenders", () => {
  const onHealthy = vi.fn();
  const el = rootWith(1);
  const view = render({ live: true, root: () => el, errors: () => 0, onHealthy });
  view.rerender({ live: true, root: () => el, errors: () => 0, onHealthy });
  view.rerender({ live: true, root: () => el, errors: () => 0, onHealthy: () => onHealthy() });
  expect(onHealthy).toHaveBeenCalledTimes(1);
});

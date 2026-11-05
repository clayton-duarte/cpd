// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { mount } from "../testing/render.tsx";
import { HealthBeacon } from "./HealthBeacon.tsx";

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function rootWith(children: number): HTMLElement {
  const el = document.createElement("div");
  for (let i = 0; i < children; i++) el.appendChild(document.createElement("section"));
  return el;
}

it("beacon reports healthy once after mount with a non-empty root and no errors", () => {
  const onHealthy = vi.fn();
  mounted = mount(<HealthBeacon root={() => rootWith(1)} errors={() => 0} onHealthy={onHealthy} />);
  expect(onHealthy).toHaveBeenCalledTimes(1);
});

it("beacon stays silent when an error was counted", () => {
  const onHealthy = vi.fn();
  mounted = mount(<HealthBeacon root={() => rootWith(1)} errors={() => 1} onHealthy={onHealthy} />);
  expect(onHealthy).not.toHaveBeenCalled();
  const control = vi.fn();
  mount(<HealthBeacon root={() => rootWith(1)} errors={() => 0} onHealthy={control} />).unmount();
  expect(control).toHaveBeenCalledTimes(1); // the same beacon does report when clean
});

it("beacon stays silent when the root is empty", () => {
  const onHealthy = vi.fn();
  mounted = mount(<HealthBeacon root={() => rootWith(0)} errors={() => 0} onHealthy={onHealthy} />);
  expect(onHealthy).not.toHaveBeenCalled();
  const control = vi.fn();
  mount(<HealthBeacon root={() => rootWith(1)} errors={() => 0} onHealthy={control} />).unmount();
  expect(control).toHaveBeenCalledTimes(1); // the same beacon does report with children
  const missing = vi.fn();
  const second = mount(<HealthBeacon root={() => null} errors={() => 0} onHealthy={missing} />);
  second.unmount();
  expect(missing).not.toHaveBeenCalled();
});

it("beacon reports once across rerenders", async () => {
  const { act } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const onHealthy = vi.fn();
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  const el = rootWith(1);
  act(() => root.render(<HealthBeacon root={() => el} errors={() => 0} onHealthy={onHealthy} />));
  act(() => root.render(<HealthBeacon root={() => el} errors={() => 0} onHealthy={onHealthy} />));
  act(() => root.render(<HealthBeacon root={() => el} errors={() => 0} onHealthy={() => onHealthy()} />));
  expect(onHealthy).toHaveBeenCalledTimes(1);
  act(() => root.unmount());
  container.remove();
});

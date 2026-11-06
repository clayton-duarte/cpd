// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { StatusBar } from "./StatusBar.tsx";
import type { StatusModel } from "./StatusBar.tsx";

const none = (): void => {};
const base: StatusModel = { runner: null, preview: null, incident: null, workers: null, version: null, hostAlert: false, mode: "production", drift: null };
let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function bar(onDocs: () => void): HTMLElement {
  const view = mount(
    <StatusBar status={base} costsOn={false} onReset={none} onLegend={none} onDocs={onDocs} onLogs={none} onCosts={none} onSettings={none} />,
  );
  mounted = view;
  return view.container;
}

it("the status bar shows Documentation between Legend and Logs when onDocs is set", () => {
  const c = bar(none);
  const names = Array.from(c.querySelectorAll("button")).map((b) => b.getAttribute("aria-label") ?? b.textContent);
  expect(names).toEqual(["Reset layout", "Legend", "Documentation", "Logs", "Costs", "Settings"]);
});

it("the Documentation button calls onDocs", () => {
  const onDocs = vi.fn();
  const button = bar(onDocs).querySelector<HTMLButtonElement>('button[data-part="docs"]');
  expect(button).not.toBeNull();
  button?.click();
  expect(onDocs).toHaveBeenCalledTimes(1);
});

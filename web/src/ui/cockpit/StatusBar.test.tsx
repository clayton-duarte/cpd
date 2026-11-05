// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { StatusBar } from "./StatusBar.tsx";
import type { StatusModel } from "./StatusBar.tsx";

const none = (): void => {};
const base: StatusModel = { runner: null, preview: null, incident: null, workers: null, version: null, hostAlert: false };
let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function bar(status: Partial<StatusModel> = {}, costsOn = false): HTMLElement {
  const view = mount(
    <StatusBar status={{ ...base, ...status }} costsOn={costsOn} onReset={none} onLegend={none} onLogs={none} onCosts={none} onSettings={none} />,
  );
  mounted = view;
  return view.container;
}
const part = (c: HTMLElement, name: string) => c.querySelector(`[data-part="${name}"]`);

it("the status bar shows the five buttons in order reset legend logs costs settings", () => {
  const c = bar();
  const names = Array.from(c.querySelectorAll("button")).map((b) => b.getAttribute("aria-label") ?? b.textContent);
  expect(names).toEqual(["Reset layout", "Legend", "Logs", "Costs", "Settings"]);
});

it("the status bar shows the runner pill when a runner is set", () => {
  expect(part(bar({ runner: "durable" }), "runner")?.textContent).toBe("durable");
  mounted?.unmount();
  expect(part(bar({ runner: null }), "runner")).toBeNull();
});

it("the status bar shows preview sha in the landing colour", () => {
  const el = part(bar({ preview: "wt abc1234" }), "preview");
  expect(el?.textContent).toBe("preview: wt abc1234");
  expect(el?.className).toContain("text-status-landing");
});

it("the status bar shows workers as Q·W·N/M", () => {
  const c = bar({ workers: { queued: 3, ready: 2, tenants: 4, concurrency: 6 } });
  expect(part(c, "workers")?.textContent).toBe("workers 3·2·4/6");
});

it("the status bar shows Incident Mode with elapsed time only while an incident is open", () => {
  expect(part(bar({ incident: "2h 5m" }), "incident")?.textContent).toBe("Incident Mode: 2h 5m");
  mounted?.unmount();
  expect(part(bar({ incident: null }), "incident")).toBeNull();
});

it("the status bar shows the version in red on a host alert", () => {
  const ok = part(bar({ version: "0.21.102" }), "version");
  expect(ok?.textContent).toBe("cpd: 0.21.102");
  expect(ok?.className).not.toContain("text-status-blocked");
  mounted?.unmount();
  const alert = part(bar({ version: "0.21.102", hostAlert: true }), "version");
  expect(alert?.getAttribute("data-alert")).toBe("true");
  expect(alert?.className).toContain("text-status-blocked");
});

// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { barStatus, StatusBar } from "./StatusBar.tsx";
import type { StatusModel } from "./StatusBar.tsx";
import type { StatusModel as WireStatus } from "../../protocol/cockpit.ts";

const none = (): void => {};
const base: StatusModel = { runner: null, preview: null, incident: null, workers: null, version: null, hostAlert: false, mode: "production", drift: null };
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
  expect(ok?.textContent).toBe("0.21.102");
  expect(ok?.className).not.toContain("text-status-blocked");
  mounted?.unmount();
  const alert = part(bar({ version: "0.21.102", hostAlert: true }), "version");
  expect(alert?.getAttribute("data-alert")).toBe("true");
  expect(alert?.className).toContain("text-status-blocked");
});

it("StatusBar shows the version alone inside a ghost badge", () => {
  const el = part(bar({ version: "0.21.102" }), "version");
  expect(el?.textContent).toBe("0.21.102");
  expect(el?.getAttribute("data-slot")).toBe("badge");
  expect(el?.getAttribute("data-variant")).toBe("ghost");
});

it("StatusBar shows no preview badge in production mode", () => {
  const c = bar({ version: "0.21.102", mode: "production" });
  expect(part(c, "version")?.getAttribute("data-variant")).toBe("ghost");
  expect(part(c, "mode")).toBeNull();
  expect(c.textContent).not.toContain("preview");
});

it("StatusBar appends a sakura outline preview badge after the version in preview mode", () => {
  const c = bar({ version: "0.21.102", mode: "preview" });
  const mode = part(c, "mode");
  expect(mode?.textContent).toBe("preview");
  expect(mode?.getAttribute("data-slot")).toBe("badge");
  expect(mode?.getAttribute("data-variant")).toBe("outline");
  expect(mode?.className).toMatch(/sakura/);
  expect(part(c, "version")?.nextElementSibling).toBe(mode);
});

it("StatusBar shows the drift count and flags it stale after 24 hours", () => {
  const fresh = part(bar({ mode: "preview", drift: { count: 3, oldestAt: "2026-11-06T10:00:00Z", stale: false } }), "drift");
  expect(fresh?.textContent).toBe("drift 3");
  expect(fresh?.hasAttribute("data-stale")).toBe(false);
  mounted?.unmount();
  const stale = part(bar({ mode: "preview", drift: { count: 2, oldestAt: "2026-11-04T10:00:00Z", stale: true } }), "drift");
  expect(stale?.textContent).toBe("drift 2");
  expect(stale?.getAttribute("data-stale")).toBe("true");
  expect(stale?.className).toMatch(/mitarashi/);
});

it("StatusBar shows no drift pill when there is no drift", () => {
  expect(part(bar({ version: "1", mode: "preview" }), "mode")?.textContent).toBe("preview");
  mounted?.unmount();
  expect(part(bar({ mode: "preview", drift: null }), "drift")).toBeNull();
  mounted?.unmount();
  expect(part(bar({ mode: "preview", drift: { count: 0, oldestAt: null, stale: false } }), "drift")).toBeNull();
});

it("barStatus carries the mode and the drift of the wire status", () => {
  const drift = { count: 1, oldestAt: "2026-11-06T10:00:00Z", stale: false };
  const wire: WireStatus = { runner: "pi", preview: null, incident: null, workers: null, version: "1", hostAlert: false, mode: "preview", drift };
  const out = barStatus(wire, 0);
  expect(out.mode).toBe("preview");
  expect(out.drift).toEqual(drift);
  const c = bar(out);
  expect(part(c, "mode")?.textContent).toBe("preview");
  expect(part(c, "drift")?.textContent).toBe("drift 1");
});

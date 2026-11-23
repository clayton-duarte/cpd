// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act } from "react";
import { mount } from "../../testing/render.tsx";
import { barStatus, StatusBar } from "./StatusBar.tsx";
import type { StatusModel } from "./StatusBar.tsx";
import type { StatusModel as WireStatus } from "../../protocol/cockpit.ts";

const none = (): void => {};
const base: StatusModel = { runner: null, preview: null, incident: null, workers: null, version: null, hostAlert: false, mode: "production", drift: null };
/** The server's status model as `barStatus` consumes it (the wire shape), with the incident carried from the serve pane. */
interface PaneStatus {
  runner: string | null;
  preview: string | null;
  incident: { startedAt: string; lead: string | null } | null;
  workers: { queued: number; waiting: number; tenants: number; limit: number } | null;
  version: string | null;
  hostAlert: boolean;
  mode: "production" | "preview";
  drift: null;
}
const basePane: PaneStatus = { runner: null, preview: null, incident: null, workers: null, version: null, hostAlert: false, mode: "production", drift: null };
let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; vi.unstubAllGlobals(); });

function bar(status: Partial<StatusModel> = {}, opts: { costsOn?: boolean; onSettings?: () => void; onSelectSession?: (sessionId: string) => void } = {}): HTMLElement {
  const view = mount(
    <StatusBar
      status={{ ...base, ...status }}
      costsOn={opts.costsOn ?? false}
      onReset={none}
      onLegend={none}
      onLogs={none}
      onCosts={none}
      onSettings={opts.onSettings ?? none}
      onSelectSession={opts.onSelectSession ?? none}
    />,
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

it("workers segment renders the TUI text from the serve status", () => {
  const pane: PaneStatus = { ...basePane, workers: { queued: 3, waiting: 2, tenants: 9, limit: 6 } };
  const c = bar(barStatus(pane, 0));
  expect(part(c, "workers")?.textContent).toBe("workers 3 queued · 2 waiting · 9/6 running");
});

it("incident pill renders elapsed minutes while incident is on", () => {
  const pane: PaneStatus = { ...basePane, incident: { startedAt: "2026-11-08T05:00:00Z", lead: "lead-1" } };
  const c = bar(barStatus(pane, Date.parse("2026-11-08T05:05:30Z")));
  expect(part(c, "incident")?.textContent).toBe("Incident Mode: 5m");
  mounted?.unmount();
  expect(part(bar(barStatus(basePane, 0)), "incident")).toBeNull();
});

it("clicking the workers segment opens Settings", () => {
  let opened = 0;
  const pane: PaneStatus = { ...basePane, workers: { queued: 1, waiting: 2, tenants: 3, limit: 4 } };
  const c = bar(barStatus(pane, 0), { onSettings: () => { opened += 1; } });
  (part(c, "workers") as HTMLElement).click();
  expect(opened).toBe(1);
});

it("clicking the incident pill selects the incident lead", () => {
  const selected: string[] = [];
  const pane: PaneStatus = { ...basePane, incident: { startedAt: "2026-11-08T05:00:00Z", lead: "lead-1" } };
  const c = bar(barStatus(pane, Date.parse("2026-11-08T05:05:30Z")), { onSelectSession: (id) => selected.push(id) });
  (part(c, "incident") as HTMLElement).click();
  expect(selected).toEqual(["lead-1"]);
});

it("the status bar shows Incident Mode with elapsed time only while an incident is open", () => {
  expect(part(bar({ incident: { elapsed: "2h 5m", lead: null } }), "incident")?.textContent).toBe("Incident Mode: 2h 5m");
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

it("copies the exact displayed version and confirms only after the write resolves", async () => {
  let resolve!: () => void;
  const writeText = vi.fn(() => new Promise<void>((done) => { resolve = done; }));
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  const c = bar({ version: "0.21.102 (abc1234)" });
  const button = part(c, "version") as HTMLButtonElement;
  expect(button.tagName).toBe("BUTTON");
  await act(async () => { button.click(); });
  expect(writeText).toHaveBeenCalledWith(button.textContent);
  expect(c.querySelector('[role="status"]')).toBeNull();
  await act(async () => { resolve(); });
  expect(c.querySelector('[role="status"]')?.textContent).toBe("Version copied");
  expect(fetch).not.toHaveBeenCalled();
  expect(document.querySelector('[data-part="inbox"]')).toBeNull();
});

it("does not confirm rejected or unavailable clipboard writes", async () => {
  vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn().mockRejectedValue(new Error("denied")) } });
  const c = bar({ version: "1+rev" });
  await act(async () => { (part(c, "version") as HTMLButtonElement).click(); });
  expect(c.querySelector('[role="status"]')).toBeNull();
  vi.stubGlobal("navigator", {});
  await act(async () => { (part(c, "version") as HTMLButtonElement).click(); });
  expect(c.querySelector('[role="status"]')).toBeNull();
});

it("uses native button keyboard activation and hides a null version", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const c = bar({ version: "1+revision" });
  const button = part(c, "version") as HTMLButtonElement;
  button.focus();
  expect(document.activeElement).toBe(button);
  // jsdom does not implement native keyboard activation; assert native button semantics,
  // then dispatch the clicks a browser generates for Enter and Space.
  for (const key of ["Enter", " "]) {
    await act(async () => {
      button.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
      button.click();
      button.dispatchEvent(new KeyboardEvent("keyup", { key, bubbles: true }));
    });
  }
  expect(writeText).toHaveBeenCalledTimes(2);
  mounted?.unmount();
  expect(part(bar({ version: null }), "version")).toBeNull();
});


it("removes the local confirmation after a brief interval", async () => {
  vi.useFakeTimers();
  try {
    vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
    const c = bar({ version: "1+rev" });
    await act(async () => { (part(c, "version") as HTMLButtonElement).click(); });
    expect(c.querySelector('[role="status"]')?.textContent).toBe("Version copied");
    act(() => { vi.advanceTimersByTime(2000); });
    expect(c.querySelector('[role="status"]')).toBeNull();
  } finally {
    vi.useRealTimers();
  }
});

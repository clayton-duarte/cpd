// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { CrashReport } from "../electron/health.ts";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { mount } from "../testing/render.tsx";
import { Root } from "./Root.tsx";

vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

const appMode = vi.hoisted(() => ({ throws: false }));
vi.mock("./App.tsx", () => ({
  App: () => { if (appMode.throws) throw new Error("app exploded"); return <main data-app>shell</main>; },
}));

const transport: Transport = {
  connect() {}, send() {}, close() {},
  onMessage: (): Disposable => ({ dispose() {} }),
  onStatus: (): Disposable => ({ dispose() {} }),
};
const base = { transport, scheduler: new FakeScheduler(), reader: { readBase64: async () => "", readText: async () => "" } };

let mounted: { unmount(): void } | null = null;
let rootEl: HTMLElement | null = null;
beforeEach(() => { vi.spyOn(console, "error").mockImplementation(() => {}); appMode.throws = false; });
afterEach(() => { mounted?.unmount(); mounted = null; rootEl?.remove(); rootEl = null; vi.restoreAllMocks(); });

/** Mounts Root inside a #root element, as index.html does. */
function mountInRoot(el: React.ReactElement) {
  rootEl = document.createElement("div");
  rootEl.id = "root";
  document.body.appendChild(rootEl);
  const view = mount(el);
  rootEl.appendChild(view.container);
  mounted = view;
  return view;
}

it("Root shows the fallback with a reload button when App throws", () => {
  appMode.throws = true;
  const reload = vi.fn();
  const view = mountInRoot(<Root {...base} reload={reload} />);
  const alert = view.container.querySelector('[role="alert"]');
  expect(alert).not.toBeNull();
  const button = [...alert!.querySelectorAll("button")].find((b) => b.textContent === "Reload");
  expect(button).toBeDefined();
  act(() => { button!.click(); });
  expect(reload).toHaveBeenCalledTimes(1);
});

it("Root reports the App crash through onCrash", () => {
  appMode.throws = true;
  const onCrash = vi.fn<(r: CrashReport) => void>();
  mountInRoot(<Root {...base} onCrash={onCrash} />);
  expect(onCrash).toHaveBeenCalledTimes(1);
  expect(onCrash.mock.calls[0]![0]).toMatchObject({ source: "error-boundary", error: "app exploded" });
});

it("Root reports healthy after the shell mounts", () => {
  const onHealthy = vi.fn();
  mountInRoot(<Root {...base} onHealthy={onHealthy} errors={() => 0} />);
  expect(onHealthy).toHaveBeenCalledTimes(1);
});

// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { CrashReport } from "../electron/health.ts";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { mount } from "../testing/render.tsx";
import { Root } from "./Root.tsx";

vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

// The App throws in the crash tests; otherwise it is the real one, so health flows through it.
const appMode = vi.hoisted(() => ({ throws: false }));
vi.mock("./App.tsx", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./App.tsx")>();
  return {
    App: (props: Parameters<typeof actual.App>[0]) => {
      if (appMode.throws) throw new Error("app exploded");
      return <actual.App {...props} />;
    },
  };
});

/** A transport whose incoming frames the test drives (the first snapshot turns health on). */
class FakeTransport implements Transport {
  private messageHandlers: ((raw: unknown) => void)[] = [];
  private statusHandlers: ((s: "open" | "closed") => void)[] = [];
  connect(): void {}
  send(): void {}
  close(): void {}
  onMessage(h: (raw: unknown) => void): Disposable {
    this.messageHandlers.push(h);
    return { dispose: () => { this.messageHandlers = this.messageHandlers.filter((x) => x !== h); } };
  }
  onStatus(h: (s: "open" | "closed") => void): Disposable {
    this.statusHandlers.push(h);
    return { dispose: () => { this.statusHandlers = this.statusHandlers.filter((x) => x !== h); } };
  }
  emitMessage(raw: unknown): void { for (const h of [...this.messageHandlers]) h(raw); }
  emitStatus(s: "open" | "closed"): void { for (const h of [...this.statusHandlers]) h(s); }
}

const transport: Transport = {
  connect() {}, send() {}, close() {},
  onMessage: (): Disposable => ({ dispose() {} }),
  onStatus: (): Disposable => ({ dispose() {} }),
};
const base = { transport, scheduler: new FakeScheduler(), reader: { readBase64: async () => "", readText: async () => "" } };

// The first snapshot of a boot (web/CONTRACT.md §15.2): an empty cockpit, but the store is live.
const snapshot = {
  type: "cockpit.snapshot",
  tree: { cursor: 1, contexts: [] },
  plan: null,
  inbox: { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 1 },
  status: null,
  costs: null,
};

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

it("Root reports healthy after the first snapshot", () => {
  const onHealthy = vi.fn();
  const fake = new FakeTransport();
  mountInRoot(<Root {...base} transport={fake} onHealthy={onHealthy} errors={() => 0} />);
  // A mounted shell with no data is not health (web/CONTRACT.md §15.2).
  expect(onHealthy).not.toHaveBeenCalled();
  // The first cockpit.snapshot turns the window healthy, once.
  act(() => { fake.emitStatus("open"); });
  act(() => { fake.emitMessage(snapshot); });
  expect(onHealthy).toHaveBeenCalledTimes(1);
});

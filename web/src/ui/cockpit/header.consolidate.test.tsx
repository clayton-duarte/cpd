// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../../protocol/runtime.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { mount } from "../../testing/render.tsx";
import { App } from "../App.tsx";
import { Header } from "./Header.tsx";
import type { BreadcrumbCrumb } from "./Header.tsx";

// jsdom has no ResizeObserver, which react-resizable-panels (external) needs: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

class FakeTransport implements Transport {
  sent: unknown[] = [];
  private messageHandlers: ((raw: unknown) => void)[] = [];
  private statusHandlers: ((s: "open" | "closed") => void)[] = [];
  connect(): void {}
  send(raw: unknown): void { this.sent.push(raw); }
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

const reader = { readBase64: async () => "QUJD", readText: async () => "text" };
const sessionRef = (id: string, key: string, role: string) => ({ id, key, runner: "durable", role, state: "idle", title: key });
const node = (id: string, key: string, role: string, planId: string | null) => ({
  ref: sessionRef(id, key, role), contextId: "ctx", planId, jobKey: null, archived: false, dot: { state: "idle", attention: null },
});
const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
const snapshot = {
  type: "cockpit.snapshot",
  tree: {
    cursor: 1,
    hostname: "mbp",
    contexts: [
      { id: "host", title: "Host", archived: false, records: 0, openQuestions: 0, plans: [], sessions: [node("hm1", "host", "manager", null)] },
      {
        id: "ctx", title: "Context One", archived: false, records: 0, openQuestions: 0,
        sessions: [node("m1", "ctx", "manager", null)],
        plans: [{ id: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", archived: false, counts, leadSessionId: "s1", sessions: [node("s1", "ctx:plan", "lead", "plan")] }],
      },
    ],
  },
  plan: { planId: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", counts, review: null, openEscalations: 0, workflows: [], jobs: [], cursor: 1 },
  inbox: { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 1 },
};

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function startApp() {
  const transport = new FakeTransport();
  const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => "c1"} />);
  mounted = view;
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot));
  const plan = view.container.querySelector<HTMLButtonElement>('button[data-select="plan"]');
  expect(plan).not.toBeNull();
  act(() => { plan!.click(); });
  return { transport, container: view.container };
}
const crumb = (c: Element, part: string) => c.querySelector<HTMLElement>(`[data-part="breadcrumb"] [data-part="${part}"]`);

it("header breadcrumb renders host, session group, manager, lead and worker crumbs", () => {
  const onClick = vi.fn();
  const crumbs: BreadcrumbCrumb[] = [
    { part: "host", label: "mbp", onClick },
    { part: "group", label: "contexts" },
    { part: "manager", label: "ctx", onClick },
    { part: "lead", label: "plan", onClick },
    { part: "worker", label: "job-a", onClick },
  ];
  const m = mount(<Header crumbs={crumbs} />);
  const nav = m.container.querySelector('nav[aria-label="Location"][data-part="breadcrumb"]');
  expect(nav).not.toBeNull();
  expect(nav?.querySelector("ol")).not.toBeNull();
  expect(nav?.querySelectorAll("li")).toHaveLength(5);
  const parts = ["breadcrumb-host", "breadcrumb-group", "breadcrumb-context", "breadcrumb-plan", "breadcrumb-worker"];
  expect(parts.map((p) => crumb(m.container, p)?.textContent)).toEqual(["mbp", "contexts", "ctx", "plan", "job-a"]);
  // a crumb with a click is a button, one without is plain text; the last one is the current page
  expect(crumb(m.container, "breadcrumb-host")?.tagName).toBe("BUTTON");
  expect(crumb(m.container, "breadcrumb-group")?.tagName).toBe("SPAN");
  expect(crumb(m.container, "breadcrumb-worker")?.getAttribute("aria-current")).toBe("page");
  expect(crumb(m.container, "breadcrumb-plan")?.getAttribute("aria-current")).toBeNull();
  expect(nav?.textContent?.split("/").length).toBe(5);
  m.unmount();
});

it("header host crumb opens the standing host manager chat", () => {
  const { transport, container } = startApp();
  expect(crumb(container, "breadcrumb-host")?.textContent).toBe("mbp");
  expect(crumb(container, "breadcrumb-group")?.textContent).toBe("contexts");
  act(() => { crumb(container, "breadcrumb-host")!.click(); });
  expect(transport.sent).toContainEqual({ type: "session.attach", sessionId: "hm1" });
});

it("header manager and lead crumbs open that session's chat", () => {
  const { transport, container } = startApp();
  expect(crumb(container, "breadcrumb-plan")?.textContent).toBe("plan");
  transport.sent.length = 0;
  act(() => { crumb(container, "breadcrumb-plan")!.click(); });
  expect(transport.sent).toContainEqual({ type: "cockpit.selectPlan", planId: "plan" });
  expect(crumb(container, "breadcrumb-plan")?.tagName).toBe("BUTTON");
  expect(crumb(container, "breadcrumb-context")?.textContent).toBe("ctx");
  act(() => { crumb(container, "breadcrumb-context")!.click(); });
  expect(transport.sent).toContainEqual({ type: "session.attach", sessionId: "m1" });
});

// @vitest-environment jsdom
import { act } from "react";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../../protocol/runtime.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { mount } from "../../testing/render.tsx";
import { App } from "../App.tsx";
import { Header } from "./Header.tsx";
import { Shell } from "./Shell.tsx";
import { StatusBar } from "./StatusBar.tsx";

// jsdom has no ResizeObserver, which react-resizable-panels (external) needs: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });


class FakeTransport implements Transport {
  sent: unknown[] = [];
  connects = 0;
  closed = false;
  private messageHandlers: ((raw: unknown) => void)[] = [];
  private statusHandlers: ((s: "open" | "closed") => void)[] = [];
  connect(): void { if (!this.closed) this.connects++; }
  send(raw: unknown): void { this.sent.push(raw); }
  close(): void { this.closed = true; }
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

const reader = {
  readBase64: async () => "QUJD",
  readText: async () => "text",
};

const ref = { id: "s1", key: "ctx:plan", runner: "durable", role: "lead", state: "idle", title: "My chat" } as const;
const node = { ref, contextId: "ctx", planId: "plan", jobKey: null, archived: false, dot: { state: "idle", attention: null } };
const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
const snapshot = {
  type: "cockpit.snapshot",
  tree: {
    cursor: 1,
    contexts: [{ id: "ctx", title: "Context One", archived: false, records: 0, openQuestions: 0, sessions: [],
      plans: [{ id: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", archived: false, counts, leadSessionId: "s1", sessions: [node] }] }],
  },
  plan: {
    planId: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", counts, review: null, openEscalations: 0,
    workflows: [], jobs: [], cursor: 1,
  },
  inbox: { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 1 },
};
const footer = {
  model: { state: "known", value: "p/m" }, tier: { state: "known", value: "S1" }, thinking: { state: "known", value: "high" },
  contextPercent: { state: "known", value: 5 }, cost: { state: "known", value: 0.1 }, branch: { state: "known", value: "main" },
};
const yes = { supported: true } as const;
const capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786432 },
  steer: yes, followUp: yes, abort: yes, abortSubmission: yes, compact: yes,
  configure: { model: yes, thinking: yes, tier: { supported: true, options: ["S1", "S2"] } },
  dialogs: { mode: "interactive" },
};

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function show(el: ReactNode): HTMLElement {
  const view = mount(<>{el}</>);
  mounted = view;
  return view.container;
}

// Shell slots `header`/`status` are not in the current prop type (pro-d18); passed loosely so the file typechecks before the implementation.
type SlotProps = { tree: ReactNode; plan: ReactNode; chat: ReactNode; inbox: ReactNode; header?: ReactNode; status?: ReactNode };
const AnyShell = Shell as unknown as (p: SlotProps) => ReactNode;
const panes = { tree: <p>T</p>, plan: <p>P</p>, chat: <p>C</p>, inbox: <p>I</p> };
const labels = (c: HTMLElement) => Array.from(c.querySelector("[data-shell]")!.children).map((e) => e.getAttribute("aria-label") ?? e.getAttribute("data-column"));

  it("Header shows the C-P-D emblem text", () => {
  const c = show(<Header />);
  expect(c.querySelector("[data-emblem]")?.textContent).toBe("C-P-D");
});

it("StatusBar shows runner, state and version", () => {
  const c = show(<StatusBar runner="pi" state="idle" version={{ state: "known", value: "0.21.102" }} />);
  expect(c.querySelector('[data-part="runner"]')?.textContent).toBe("Runner: pi");
  expect(c.querySelector('[data-part="state"]')?.textContent).toBe("State: idle");
  expect(c.querySelector('[data-part="version"]')?.textContent).toBe("cpd: 0.21.102");
});

it("StatusBar shows unknown version and none for no session", () => {
  const c = show(<StatusBar runner={null} state={null} version={{ state: "unknown", reason: "r" }} />);
  expect(c.querySelector('[data-part="runner"]')?.textContent).toBe("Runner: none");
  expect(c.querySelector('[data-part="state"]')?.textContent).toBe("State: none");
  expect(c.querySelector('[data-part="version"]')?.textContent).toBe("cpd: unknown");
  mounted?.unmount();
  const c2 = show(<StatusBar runner={null} state={null} version={null} />);
  expect(c2.querySelector('[data-part="version"]')?.textContent).toBe("cpd: unknown");
});

it("Shell renders header and status slots around the panes", () => {
  const c = show(<AnyShell {...panes} header={<span>H</span>} status={<span>S</span>} />);
  expect(labels(c)).toEqual(["Cockpit header", "Context tree column", "center", "Inbox column", "Status bar"]);
  const shell = c.querySelector("[data-shell]")!;
  expect(shell.firstElementChild?.tagName).toBe("HEADER");
  expect(shell.firstElementChild?.textContent).toBe("H");
  expect(shell.lastElementChild?.tagName).toBe("FOOTER");
  expect(shell.lastElementChild?.textContent).toBe("S");
  expect(shell.querySelector('[data-column="center"]')!.children.length).toBe(2);
});

it("Shell without header or status renders as before", () => {
  const c = show(<AnyShell {...panes} />);
  expect(labels(c)).toEqual(["Context tree column", "center", "Inbox column"]);
  expect(c.querySelector("header, footer")).toBeNull();
  mounted?.unmount();
  const c2 = show(<AnyShell {...panes} header={<span>H</span>} />);
  expect(labels(c2)).toEqual(["Cockpit header", "Context tree column", "center", "Inbox column"]);
});

it("App shows the header and the status bar with the selected session runner", () => {
  const transport = new FakeTransport();
  const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => "c1"} />);
  mounted = view;
  const container = view.container;
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot));
  expect(container.querySelector('header[aria-label="Cockpit header"] [data-emblem]')?.textContent).toBe("C-P-D");
  expect(container.querySelector('footer[aria-label="Status bar"] [data-part="runner"]')?.textContent).toBe("Runner: none");
  act(() => { container.querySelector<HTMLButtonElement>('[data-session="s1"]')!.click(); });
  act(() => {
    transport.emitMessage({ type: "session.attached", sessionId: "s1", capabilities, title: "My chat", meta: { ref, footer } });
  });
  const bar = container.querySelector('footer[aria-label="Status bar"]')!;
  expect(bar.querySelector('[data-part="runner"]')?.textContent).toBe("Runner: durable");
  expect(bar.querySelector('[data-part="state"]')?.textContent).toBe("State: idle");
  expect(bar.querySelector('[data-part="version"]')?.textContent).toBe("cpd: unknown");
});

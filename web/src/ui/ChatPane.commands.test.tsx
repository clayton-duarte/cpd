// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { ContextTreeModel, SessionNode } from "../protocol/cockpit.ts";
import type { Capabilities, SessionRef } from "../protocol/domain.ts";
import type { Connection, SubmitInput } from "../state/connection.ts";
import { emptyTranscript } from "../state/transcript.ts";
import type { SessionView } from "../state/store.ts";
import { mount } from "../testing/render.tsx";
import { ChatPane } from "./ChatPane.tsx";
import type { FileReaderBoundary } from "./composer/attachments.ts";

vi.mock("@base-ui/react/scroll-area", () => import("../testing/scroll-area-mock.tsx"));

vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

const yes = { supported: true } as const;
const capabilities: Capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786_432 },
  steer: yes, followUp: yes, abort: yes, abortSubmission: yes, compact: yes,
  configure: { model: yes, thinking: yes, tier: { supported: true, options: ["S1"] } },
  dialogs: { mode: "interactive" },
};
const reader: FileReaderBoundary = { readBase64: async () => "QUJD", readText: async () => "text" };

const session = (id: string, role: SessionRef["role"], planId: string | null = null): SessionNode => ({
  ref: { id, key: id, runner: "durable", role, state: "idle", title: id },
  contextId: "ctx", planId, jobKey: null, archived: false, dot: { state: "idle", attention: null },
});
const tree: ContextTreeModel = {
  cursor: 1,
  contexts: [{
    id: "ctx", title: "Context", archived: false, records: 0, openQuestions: 0,
    sessions: [session("mgr", "manager")],
    plans: [{
      id: "plan", contextId: "ctx", title: "Plan", phase: "dispatch", archived: false,
      counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
      leadSessionId: "lead", sessions: [session("lead", "lead", "plan")],
    }],
  }],
};

const view = (id: string, role: SessionRef["role"], archived = false): SessionView => ({
  id, attached: true,
  ref: { id, key: id, runner: "durable", role, state: "idle", title: id },
  title: id, footer: null, capabilities, transcript: emptyTranscript, busy: false,
  queued: [], pending: [], dialog: null, models: null, error: null, archived,
});

class FakeConnection implements Connection {
  submits: SubmitInput[] = [];
  actions: unknown[] = [];
  start(): void {}
  selectSession(): void {}
  selectPlan(): void {}
  loadEarlier(): void {}
  submit(input: SubmitInput): string { this.submits.push(input); return "c1"; }
  abort(): void {}
  abortSubmission(): void {}
  compact(): void {}
  configure(): void {}
  listModels(): void {}
  respond(): void {}
  action(action: unknown): string { this.actions.push(action); return "a1"; }
  readLogs(): void {}
  readSettings(): void {}
  saveSettings(): void {}
  close(): void {}
}

const mounted: { unmount(): void }[] = [];
afterEach(() => {
  for (const m of mounted.splice(0)) m.unmount();
  vi.restoreAllMocks();
});

function setup(v: SessionView) {
  const connection = new FakeConnection();
  const m = mount(<ChatPane view={v} connection={connection} reader={reader} tree={tree} />);
  mounted.push(m);
  return { m, connection };
}
const textarea = (c: HTMLElement) => c.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
function typeText(el: HTMLTextAreaElement, value: string) {
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function pressEnter(el: Element) {
  act(() => { el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); });
}

it("Manager /archive dispatches the Context archive action without submitting a turn", () => {
  const { m, connection } = setup(view("mgr", "manager"));
  typeText(textarea(m.container), "/archive");
  pressEnter(textarea(m.container));
  expect(connection.actions).toEqual([{ kind: "context.archive", contextId: "ctx" }]);
  expect(connection.submits).toEqual([]);
});

it("Lead /archive dispatches the Plan archive action without submitting a turn", () => {
  const { m, connection } = setup(view("lead", "lead"));
  typeText(textarea(m.container), "/archive");
  pressEnter(textarea(m.container));
  expect(connection.actions).toEqual([{ kind: "plan.archive", planId: "plan" }]);
  expect(connection.submits).toEqual([]);
});

it("an archived Manager chat accepts /unarchive and dispatches the recursive Context action", () => {
  const { m, connection } = setup(view("mgr", "manager", true));
  const t = textarea(m.container);
  expect(t.disabled).toBe(false);
  typeText(t, "/unarchive");
  pressEnter(t);
  expect(connection.actions).toEqual([{ kind: "context.unarchive", contextId: "ctx" }]);
  expect(connection.submits).toEqual([]);
});

it("an archived chat does not submit ordinary text or /archive", () => {
  const { m, connection } = setup(view("mgr", "manager", true));
  const t = textarea(m.container);
  typeText(t, "hello");
  pressEnter(t);
  typeText(t, "/archive");
  pressEnter(t);
  expect(connection.actions).toEqual([]);
  expect(connection.submits).toEqual([]);
});

it("a recognized command with arguments shows an error instead of reaching the agent", () => {
  const { m, connection } = setup(view("mgr", "manager"));
  typeText(textarea(m.container), "/archive now");
  pressEnter(textarea(m.container));
  expect(connection.actions).toEqual([]);
  expect(connection.submits).toEqual([]);
  expect(m.container.querySelector('[data-part="chat-command-error"]')?.textContent).toContain("no arguments");
});

it("unknown slash commands still reach the agent", () => {
  const { m, connection } = setup(view("mgr", "manager"));
  typeText(textarea(m.container), "/review");
  pressEnter(textarea(m.container));
  expect(connection.actions).toEqual([]);
  expect(connection.submits).toHaveLength(1);
  expect(connection.submits[0]?.text).toBe("/review");
});

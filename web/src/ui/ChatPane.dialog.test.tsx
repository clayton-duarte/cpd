// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Capabilities, Dialog } from "../protocol/domain.ts";
import type { Connection, SubmitInput } from "../state/connection.ts";
import { emptyTranscript } from "../state/transcript.ts";
import type { SessionView } from "../state/store.ts";
import { mount } from "../testing/render.tsx";
import { dialogFromRequest } from "../server/durable/frames.ts";
import { ChatPane } from "./ChatPane.tsx";
import { createConnection } from "../state/connection.ts";
import { createStore } from "../state/store.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { createHubConnection, type HubDeps } from "../server/hub/connection.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import type { FileReaderBoundary } from "./composer/attachments.ts";

vi.mock("@base-ui/react/scroll-area", () => import("../testing/scroll-area-mock.tsx"));

// jsdom has no ResizeObserver, which ChatPane's virtualised transcript (external) may need: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

const yes = { supported: true } as const;
const capabilities: Capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786_432 },
  steer: yes, followUp: yes, abort: yes, abortSubmission: yes, compact: yes,
  configure: { model: yes, thinking: yes, tier: { supported: true, options: ["S1", "S2"] } },
  dialogs: { mode: "interactive" },
};
const reader: FileReaderBoundary = { readBase64: async () => "QUJD", readText: async () => "text" };

const baseView: SessionView = {
  id: "s1", attached: true,
  ref: { id: "s1", key: "ctx:plan", runner: "durable", role: "lead", state: "idle", title: "My chat" },
  title: "My chat", footer: null, capabilities, transcript: emptyTranscript, busy: false,
  queued: [], pending: [], dialog: null, models: null, error: null,
};

class FakeConnection implements Connection {
  submits: SubmitInput[] = [];
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
  action(): string { return "a1"; }
  readLogs(): void {}
  readSettings(): void {}
  saveSettings(): void {}
  close(): void {}
}


type Ask = Extract<Dialog, { method: "ask_user_question" }>;
type Plan = Extract<Dialog, { method: "cpd_plan_review" }>;
const ask = (interactive = true): Ask => ({
  id: "d1", method: "ask_user_question", interactive,
  questions: [{ header: "H1", question: "Question 1?", options: [{ label: "A", description: "a" }, { label: "B", description: "b" }] }],
});
const plan: Plan = { id: "review:r1", method: "cpd_plan_review", interactive: true, title: "Ship it", summary: "steps" };
const withDialog = (dialog: Dialog | null): SessionView => ({ ...baseView, dialog });

const connection = new (class implements Connection {
  start(): void {}
  selectSession(): void {}
  selectPlan(): void {}
  loadEarlier(): void {}
  submit(_input: SubmitInput): string { return "c1"; }
  abort(): void {}
  abortSubmission(): void {}
  compact(): void {}
  configure(): void {}
  listModels(): void {}
  respond(): void {}
  action(): string { return "a1"; }
  readLogs(): void {}
  readSettings(): void {}
  saveSettings(): void {}
  close(): void {}
})();

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function setup(dialog: Dialog | null) {
  const m = mount(<ChatPane view={withDialog(dialog)} connection={connection} reader={reader} />);
  mounted = m;
  return m.container;
}
const composerOf = (c: HTMLElement) => c.querySelector('[data-part="composer"]');
const askOf = (c: HTMLElement) => c.querySelector("[data-question]");
const planOf = (c: HTMLElement) => c.querySelector('[data-part="decision-bar"]');

it("ChatPane shows the Composer and no dialog card when view.dialog is null", () => {
  const c = setup(null);
  expect(composerOf(c)).not.toBeNull();
  expect(askOf(c)).toBeNull();
  expect(planOf(c)).toBeNull();
});

it("ChatPane replaces the Composer with the AskCard while an ask_user_question dialog is open", () => {
  const c = setup(ask());
  expect(askOf(c)).not.toBeNull();
  expect(composerOf(c)).toBeNull();
  expect(c.querySelector('textarea[aria-label="Message"]')).toBeNull();
});

it("ChatPane replaces the Composer with the PlanReviewCard while a plan review dialog is open", () => {
  const c = setup(plan);
  expect(planOf(c)).not.toBeNull();
  expect(composerOf(c)).toBeNull();
  expect(c.querySelector('textarea[aria-label="Message"]')).toBeNull();
});

it("ChatPane replaces the Composer with a read-only dialog card too", () => {
  const c = setup(ask(false));
  expect(c.querySelector('[data-dialog="ask_user_question"]')).not.toBeNull();
  expect(composerOf(c)).toBeNull();
});

it("ChatPane shows the Composer again once the dialog is resolved", () => {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  try {
    act(() => root.render(<ChatPane view={withDialog(ask())} connection={connection} reader={reader} />));
    expect(composerOf(container)).toBeNull();
    act(() => root.render(<ChatPane view={withDialog(null)} connection={connection} reader={reader} />));
    expect(composerOf(container)).not.toBeNull();
    expect(askOf(container)).toBeNull();
  } finally {
    act(() => root.unmount());
    container.remove();
  }
});

it("dropping files on ChatPane while a dialog is open does not throw", async () => {
  const c = setup(ask());
  expect(composerOf(c)).toBeNull();
  const zone = c.querySelector("[data-chat-dropzone]")!;
  const drop = new Event("drop", { bubbles: true, cancelable: true });
  Object.defineProperty(drop, "dataTransfer", { value: { files: [{ name: "a.png", type: "image/png", size: 3 }], types: ["Files"] } });
  await act(async () => { zone.dispatchEvent(drop); });
  expect(askOf(c)).not.toBeNull();
  expect(composerOf(c)).toBeNull();
});

// The durable request must map to the same answerable approval card as other approvals.
it("ChatPane swaps composer for dialog card when session is waiting on dialog", () => {
  const mapped = dialogFromRequest({ type: "ui_request", id: "external-1", created_at: 0, method: "select", purpose: "external_approve", title: "Publish release?", options: ["Allow once", "Deny"] }, []);
  expect(mapped).not.toHaveProperty("kind", "unsupported");
  if ("kind" in mapped) throw new Error("external approval must be a dialog");
  const c = setup(mapped);
  expect(composerOf(c)).toBeNull();
  expect(c.textContent).toContain("Publish release?");
  expect(c.textContent).toContain("Allow once");
  expect(c.textContent).toContain("Deny");
});

function abortSetup(dialog: Dialog | null) {
  const abort = vi.fn();
  const respond = vi.fn();
  const conn: Connection = Object.assign(new FakeConnection(), { abort, respond });
  const m = mount(<ChatPane view={{ ...withDialog(dialog), busy: true }} connection={conn} reader={reader} />);
  mounted = m;
  return { c: m.container, abort, respond };
}
const esc = () => new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });

it("Escape in a dialog aborts the running turn", () => {
  const { c, abort } = abortSetup(ask());
  const target = c.querySelector('[data-dialog="ask_user_question"]')!;
  act(() => { target.dispatchEvent(esc()); });
  expect(abort).toHaveBeenCalledTimes(1);
});

it("Escape in the composer aborts the running turn", () => {
  const { c, abort } = abortSetup(null);
  // Focus is outside the composer textarea (the pane itself), no dialog shown.
  const pane = c.querySelector("[data-chat-dropzone]")!;
  act(() => { pane.dispatchEvent(esc()); });
  expect(abort).toHaveBeenCalledTimes(1);
});

const transcriptOf = (c: HTMLElement) => c.querySelector('[data-part="transcript"]');
const headerOf = (c: HTMLElement) => c.querySelector('[data-part="chat-header"], header');

it("plan review hides the transcript and the card fills the pane", () => {
  const c = setup(plan);
  expect(transcriptOf(c)).toBeNull();
  expect(headerOf(c)).not.toBeNull();
  const card = c.querySelector<HTMLElement>('section[data-dialog="cpd_plan_review"]')!;
  expect(card).not.toBeNull();
  expect(card.classList.contains("flex-1")).toBe(true);
  expect(card.classList.contains("min-h-0")).toBe(true);
});

it("transcript returns when the plan review resolves", () => {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  try {
    act(() => root.render(<ChatPane view={withDialog(plan)} connection={connection} reader={reader} />));
    expect(transcriptOf(container)).toBeNull();
    act(() => root.render(<ChatPane view={withDialog(null)} connection={connection} reader={reader} />));
    expect(transcriptOf(container)).not.toBeNull();
    expect(composerOf(container)).not.toBeNull();
    expect(planOf(container)).toBeNull();
  } finally {
    act(() => root.unmount());
    container.remove();
  }
});

it("other dialogs keep the transcript", () => {
  const c = setup(ask());
  expect(transcriptOf(c)).not.toBeNull();
  expect(askOf(c)).not.toBeNull();
  mounted?.unmount();
  // Contrast: only the plan review replaces the transcript.
  const p = setup(plan);
  expect(transcriptOf(p)).toBeNull();
});

it("typing a custom answer survives dialog re-renders between keystrokes and is delivered verbatim", () => {
  const responses: unknown[] = [];
  const conn: Connection = Object.assign(new FakeConnection(), {
    respond: (id: string, response: unknown): void => { responses.push([id, response]); },
  });
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  // Each render hands ChatPane a fresh, equal dialog object, as store updates do.
  const render = () => act(() => root.render(<ChatPane view={withDialog(ask())} connection={conn} reader={reader} />));
  render();
  const text = container.querySelector<HTMLInputElement>('input[type="text"][aria-label="Type something."]');
  if (text === null) throw new Error("no custom field");
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  const typed = "my own answer";
  act(() => text.focus());
  for (let n = 1; n <= typed.length; n++) {
    act(() => {
      set?.call(text, typed.slice(0, n));
      text.dispatchEvent(new Event("input", { bubbles: true }));
    });
    render();
  }
  expect(container.querySelector<HTMLInputElement>('input[type="text"][aria-label="Type something."]')).toBe(text);
  expect(text.value).toBe(typed);
  const submit = container.querySelector<HTMLButtonElement>('[data-slot="questionnaire-submit"]');
  expect(submit?.getAttribute("data-status")).toBe("answered");
  act(() => submit?.click());
  expect(responses).toEqual([
    ["d1", { method: "ask_user_question", outcome: "answered", result: {
      answers: [{ questionIndex: 0, question: "Question 1?", kind: "custom", answer: typed }], cancelled: false } }],
  ]);
  act(() => root.unmount());
  container.remove();
});

it("a custom answer typed in the card reaches the backend respond through the real client frame and hub", async () => {
  const sent: unknown[] = [];
  const listeners: Array<(m: unknown) => void> = [];
  const statuses: Array<(s: "open" | "closed") => void> = [];
  const transport = {
    connect(): void {},
    send: (raw: unknown): void => { sent.push(JSON.parse(JSON.stringify(raw))); },
    close(): void {},
    onMessage: (h: (m: unknown) => void) => { listeners.push(h); return { dispose(): void {} }; },
    onStatus: (h: (s: "open" | "closed") => void) => { statuses.push(h); return { dispose(): void {} }; },
  };
  const store = createStore();
  const real = createConnection({ transport, scheduler: new FakeScheduler(), store, newId: () => "id1" });
  real.start();
  act(() => { for (const h of statuses) h("open"); });
  real.selectSession("pi-1");
  // The client holds a session's outbox entries until the hub confirms the attach.
  act(() => { for (const h of listeners) h({ type: "session.attached", sessionId: "pi-1", capabilities: {}, meta: {} }); });
  const questions = [
    { header: "H1", question: "Question 1?", options: [{ label: "A", description: "a" }, { label: "B", description: "b" }] },
    { header: "H2", question: "Question 2?", multiSelect: true, options: [{ label: "C", description: "c" }, { label: "D", description: "d" }] },
  ];
  const dialog: Ask = { id: "call:q1", method: "ask_user_question", interactive: true, questions };
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => root.render(<ChatPane view={withDialog(dialog)} connection={real} reader={reader} />));
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  const typeInto = (i: number, text: string) => {
    const el = container.querySelector<HTMLInputElement>(`fieldset[data-question="${i}"] input[type="text"][aria-label="Type something."]`);
    if (el === null) throw new Error(`no custom field ${i}`);
    act(() => { el.focus(); set?.call(el, text); el.dispatchEvent(new Event("input", { bubbles: true })); });
  };
  const slot = (name: string) => container.querySelector<HTMLButtonElement>(`[data-slot="${name}"]:not([hidden])`);
  typeInto(0, "custom one");
  act(() => slot("questionnaire-next")?.click());
  typeInto(1, "custom two");
  act(() => slot("questionnaire-submit")?.click());
  act(() => root.unmount());
  container.remove();
  const frame = sent.find((m) => (m as { type?: string }).type === "dialog.respond");
  expect(frame).toBeDefined();

  const calls: unknown[] = [];
  const errors: ServerMessage[] = [];
  const handle = { capabilities: {}, respond: (id: string, r: unknown) => { calls.push([id, r]); return Promise.resolve(); }, dispose: () => Promise.resolve() };
  const hub = createHubConnection({
    cockpit: { ready: () => Promise.resolve(), snapshot: () => ({}), selectPlan: () => Promise.resolve(), subscribe: () => ({ dispose(): void {} }), runAction: () => Promise.resolve({}) } as unknown as HubDeps["cockpit"],
    registry: { resolve: (id: string) => ({ ref: { id, key: id, runner: "pi", role: "worker", state: "idle", title: null }, file: "/f.jsonl", tierOptions: [] }) } as unknown as HubDeps["registry"],
    backends: { pi: { attach: () => Promise.resolve(handle) }, durable: {} } as unknown as HubDeps["backends"],
    send: (m) => { errors.push(m); },
  });
  await hub.handle({ type: "session.attach", sessionId: "pi-1" });
  await hub.handle(frame);
  expect(errors.filter((m) => m.type === "session.error")).toEqual([]);
  expect(calls).toEqual([
    ["call:q1", { method: "ask_user_question", outcome: "answered", result: { answers: [
      { questionIndex: 0, question: "Question 1?", kind: "custom", answer: "custom one" },
      { questionIndex: 1, question: "Question 2?", kind: "multi", selected: [], answer: "custom two" },
    ], cancelled: false } }],
  ]);
});

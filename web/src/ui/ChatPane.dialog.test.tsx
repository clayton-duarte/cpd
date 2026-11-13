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

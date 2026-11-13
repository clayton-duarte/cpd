// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import type { Dialog } from "../../protocol/domain.ts";
import type { Capabilities } from "../../protocol/domain.ts";
import type { Connection, SubmitInput } from "../../state/connection.ts";
import type { SessionView } from "../../state/store.ts";
import { emptyTranscript } from "../../state/transcript.ts";
import { mount } from "../../testing/render.tsx";
import { ChatPane } from "../ChatPane.tsx";
import type { FileReaderBoundary } from "../composer/attachments.ts";
import { PlanReviewCard, type PlanReviewExtras } from "./PlanReviewCard.tsx";

vi.mock("@base-ui/react/scroll-area", () => import("../../testing/scroll-area-mock.tsx"));
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

type Plan = Extract<Dialog, { method: "cpd_plan_review" }>;
const plan: Plan = { id: "review:r1", method: "cpd_plan_review", interactive: true, title: "Ship it", summary: "steps" };

const job = (name: string, overlap = false) => ({ name, role: "worker", tier: "S1", ...(overlap ? { overlap } : {}) });
const review: PlanReviewExtras = {
  workflows: [
    { name: "alpha", route: "main", status: "pending", summary: "first", jobs: [job("a1"), job("a2")] },
    { name: "beta", route: "main", status: "pending", summary: "second", jobs: [job("b1"), job("b2")] },
    { name: "gamma", route: "main", status: "pending", summary: "third", jobs: [job("g1"), job("g2")] },
  ],
  overlaps: ["alpha and beta touch one file"],
  risks: ["a risk"],
};

const yes = { supported: true } as const;
const capabilities: Capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786_432 },
  steer: yes, followUp: yes, abort: yes, abortSubmission: yes, compact: yes,
  configure: { model: yes, thinking: yes, tier: { supported: true, options: ["S1", "S2"] } },
  dialogs: { mode: "interactive" },
};
const reader: FileReaderBoundary = { readBase64: async () => "QUJD", readText: async () => "text" };
const view: SessionView = {
  id: "s1", attached: true,
  ref: { id: "s1", key: "ctx:plan", runner: "durable", role: "lead", state: "idle", title: "My chat" },
  title: "My chat", footer: null, capabilities, transcript: emptyTranscript, busy: false,
  queued: [], pending: [], dialog: plan, models: null, error: null,
};
const connection: Connection = {
  start() {}, selectSession() {}, selectPlan() {}, loadEarlier() {},
  submit(_i: SubmitInput) { return "c1"; },
  abort() {}, abortSubmission() {}, compact() {}, configure() {}, listModels() {},
  respond() {}, action() { return "a1"; }, readLogs() {}, readSettings() {}, saveSettings() {}, close() {},
} as Connection;

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function inPane() {
  const m = mount(<ChatPane view={view} connection={connection} reader={reader} />);
  mounted = m;
  const pane = m.container.querySelector<HTMLElement>("[data-chat-dropzone]")!;
  const section = m.container.querySelector<HTMLElement>('section[data-dialog="cpd_plan_review"]')!;
  expect(pane).not.toBeNull();
  expect(section).not.toBeNull();
  return { pane, section };
}

function standalone() {
  const m = mount(<PlanReviewCard dialog={plan} readOnly={false} onRespond={() => {}} review={review} />);
  mounted = m;
  return m.container.querySelector<HTMLElement>('section[data-dialog="cpd_plan_review"]')!;
}

const tokens = (e: Element) => Array.from(e.classList);
const isBorder = (t: string) => t === "border" || t.startsWith("border-");
const isScroller = (e: Element) =>
  e.getAttribute("data-slot") === "scroll-area-viewport" ||
  tokens(e).some((t) => t === "overflow-y-auto" || t === "overflow-auto");

it("plan review root is not an Item card", () => {
  const { section } = inPane();
  const t = tokens(section);
  expect(section.getAttribute("data-slot")).not.toBe("item");
  expect(t.filter(isBorder)).toEqual([]);
  expect(t.filter((x) => x.startsWith("rounded"))).toEqual([]);
  expect(t.filter((x) => /^mx-|^mb-/.test(x))).toEqual([]);
  for (const need of ["flex", "flex-col", "min-h-0", "flex-1"]) expect(t).toContain(need);
  expect(t).not.toContain("flex-wrap");
});

it("plan review has one scroll region and fixed header and actions", () => {
  const section = standalone();
  const scrollers = [section, ...Array.from(section.querySelectorAll("*"))].filter(isScroller);
  expect(scrollers).toHaveLength(1);
  const scroller = scrollers[0]!;
  const body = section.querySelector('[data-part="body"]')!;
  expect(scroller.contains(body)).toBe(true);
  const rows = Array.from(scroller.querySelectorAll('[data-part="workflow"]'));
  expect(rows).toHaveLength(3);
  for (const name of ["a1", "a2", "b1", "b2", "g1", "g2"]) expect(scroller.textContent).toContain(name);
  const header = section.querySelector<HTMLElement>('[data-part="header"]')!;
  expect(header).not.toBeNull();
  expect(scroller.contains(header)).toBe(false);
  expect(tokens(header)).toContain("shrink-0");
  const actions = section.querySelector<HTMLElement>('[data-part="actions"]')!;
  expect(actions).not.toBeNull();
  expect(scroller.contains(actions)).toBe(false);
  expect(tokens(actions)).toContain("shrink-0");
  for (const a of ["approve", "reject"]) {
    const b = section.querySelector(`button[data-action="${a}"]`)!;
    expect(b).not.toBeNull();
    expect(scroller.contains(b)).toBe(false);
  }
});

it("plan review scroller ancestors can shrink", () => {
  const { pane, section } = inPane();
  const scroller = [section, ...Array.from(section.querySelectorAll("*"))].find(isScroller);
  expect(scroller).toBeDefined();
  const bounded = tokens(scroller!).some((t) => /^(h|max-h)-/.test(t) && t !== "h-full");
  if (bounded) return;
  const slot = (e: Element) => (e.getAttribute("data-slot") ?? "").startsWith("scroll-area");
  for (let e = scroller!.parentElement; e && e !== pane; e = e.parentElement) {
    if (slot(e)) continue; // ScrollArea parts are the scroller itself
    const t = tokens(e);
    expect(t, `min-h-0 on <${e.tagName.toLowerCase()} class="${e.className}">`).toContain("min-h-0");
    expect(t.includes("flex-col") || (t.includes("flex") && !t.includes("flex-wrap")), `flex on ${e.className}`).toBe(true);
    expect(t, `flex-wrap on ${e.className}`).not.toContain("flex-wrap");
    if (e.getAttribute("data-part") !== "body-row") expect(t, `flex-col on ${e.className}`).toContain("flex-col");
  }
});

it("plan review has no inner border box in the pane", () => {
  const { pane, section } = inPane();
  const offenders: string[] = [];
  for (let e: Element | null = section; e && e !== pane; e = e.parentElement) {
    if (tokens(e).some(isBorder)) offenders.push(`${e.tagName.toLowerCase()}.${e.className}`);
  }
  expect(offenders).toEqual([]);
});

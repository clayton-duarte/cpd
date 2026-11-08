// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../../protocol/runtime.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { mount } from "../../testing/render.tsx";
import { App } from "../App.tsx";
import { LegendDialog } from "./LegendDialog.tsx";
import { LogsDialog } from "./LogsDialog.tsx";
import type { LogEvent } from "./LogsDialog.tsx";
import { SettingsDialog } from "./SettingsDialog.tsx";
import type { SettingsRead } from "./SettingsDialog.tsx";

// Radix (external) needs these in jsdom: inert doubles.
beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  Element.prototype.scrollIntoView = vi.fn();
  Element.prototype.hasPointerCapture = vi.fn(() => false);
});
afterEach(() => vi.unstubAllGlobals());

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });
const show = (el: React.ReactElement) => { mounted = mount(el); return mounted; };
const body = document.body;
const click = (el: Element | null) => {
  if (el === null) throw new Error("element not found");
  act(() => { el.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
};

const clock = (at: string) => {
  const d = new Date(at);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
};
const ev = (id: number, kind: string, text: string, at = "2026-01-02T03:04:05.000Z"): LogEvent => ({ id, at, kind, text });

it("the legend dialog lists its sections in order", () => {
  show(<LegendDialog open onClose={vi.fn()} />);
  const dialog = body.querySelector('[data-part="legend-dialog"]');
  const headings = Array.from(dialog?.querySelectorAll("h2, h3, h4") ?? []).map((h) => h.textContent);
  const want = [
    "Meanings → Colour", "Jobs: Plan Pills And Context Counters", "Session Dots", "Workflows (Dispatch)", "Inbox", "Pulse", "PR Checks",
  ];
  const got = want.map((w) => headings.findIndex((h) => h?.includes(w) === true));
  expect(got.every((i) => i >= 0)).toBe(true);
  expect([...got].sort((a, b) => a - b)).toEqual(got);
});

it("the logs dialog renders time and text rows with tone classes", () => {
  show(<LogsDialog open onClose={vi.fn()} events={[ev(1, "info", "hello"), ev(2, "error", "boom"), ev(3, "note", "")]} />);
  const rows = Array.from(body.querySelectorAll('[data-part="logs-dialog"] [data-part="log-row"]'));
  expect(rows.map((r) => r.textContent)).toEqual([
    `${clock("2026-01-02T03:04:05.000Z")} hello`,
    `${clock("2026-01-02T03:04:05.000Z")} boom`,
    `${clock("2026-01-02T03:04:05.000Z")} note`,
  ]);
  expect(rows[0]?.querySelector('[data-part="log-time"]')?.className).toContain("text-fg-dim");
  expect(rows[1]?.className).toContain("text-status-blocked");
  expect(rows[0]?.className).not.toContain("text-status-blocked");
});

it("the logs dialog shows none yet when empty", () => {
  show(<LogsDialog open onClose={vi.fn()} events={[]} />);
  const el = Array.from(body.querySelectorAll('[data-part="logs-dialog"] *')).find((e) => e.textContent === "none yet");
  expect(el?.className).toContain("text-fg-dim");
});

it("the logs dialog follows the bottom for new rows", () => {
  const scroll = vi.fn();
  Element.prototype.scrollIntoView = scroll;
  const first = [ev(1, "info", "a")];
  const m = show(<LogsDialog open onClose={vi.fn()} events={first} />);
  scroll.mockClear();
  act(() => { m.unmount(); });
  mounted = mount(<LogsDialog open onClose={vi.fn()} events={first} />);
  scroll.mockClear();
  const next = [...first, ev(2, "info", "b")];
  act(() => { mounted?.unmount(); });
  mounted = mount(<LogsDialog open onClose={vi.fn()} events={next} />);
  expect(body.querySelectorAll('[data-part="log-row"]').length).toBe(2);
  expect(scroll).toHaveBeenCalled();
});

const read: SettingsRead = {
  tiers: { auxiliar: "GX", worker: "H3", lead: "S3", manager: "O3" },
  experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true },
};

it("the settings dialog is titled Agent Tiers with both sections and its buttons", () => {
  show(<SettingsDialog open read={read} onSave={vi.fn()} onCancel={vi.fn()} />);
  const dialog = body.querySelector('[data-part="agent-tiers-dialog"]');
  expect(dialog?.querySelector('[data-slot="dialog-title"]')?.textContent).toBe("Agent Tiers");
  const headings = Array.from(dialog?.querySelectorAll("h3, h4") ?? []).map((h) => h.textContent);
  expect(headings).toEqual(expect.arrayContaining(["Models + Tiers", "Experimental"]));
  const buttons = Array.from(dialog?.querySelectorAll("button") ?? []).map((b) => b.textContent);
  expect(buttons).toEqual(expect.arrayContaining(["Save", "Cancel", "Load"]));
});

it("the settings dialog saves only changed values", () => {
  const onSave = vi.fn();
  show(<SettingsDialog open read={read} onSave={onSave} onCancel={vi.fn()} />);
  const sw = body.querySelector('[role="switch"][aria-label="write_curation"]');
  click(sw);
  const save = Array.from(body.querySelectorAll("button")).find((b) => b.textContent === "Save") ?? null;
  click(save);
  expect(onSave).toHaveBeenCalledExactlyOnceWith({ experimental: { write_curation: false } });
});

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

it("the Agent Tiers Save shows a refusal as the error toast with the whole message", () => {
  // The CLI's whole refusal text, uncut: the toast must carry every line, not a first line or a truncated one.
  const refusal = "the following required arguments were not provided:\n  --auxiliar <T>\n  --manager <T>\n\nUsage: cpd config tiers set --auxiliar <T> --worker <T> --lead <T> --manager <T>";
  const transport = new FakeTransport();
  const reader = { readBase64: async () => "", readText: async () => "" };
  mounted = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => "c1"} />);
  act(() => transport.emitStatus("open"));
  click(body.querySelector('[data-part="settings"]'));
  act(() => transport.emitMessage({ type: "settings.result", settings: read }));
  const dialog = body.querySelector('[data-part="agent-tiers-dialog"]');
  expect(dialog).not.toBeNull();
  click(Array.from(dialog?.querySelectorAll("button") ?? []).find((b) => b.textContent === "Save") ?? null);
  expect(transport.sent).toContainEqual({ type: "settings.save", changes: {} });
  act(() => transport.emitMessage({ type: "settings.saved", ok: false, error: { code: "cpd_failed", message: refusal } }));
  const toasts = body.querySelectorAll('[data-part="cockpit-error-toast"]');
  expect(toasts).toHaveLength(1);
  const toast = toasts[0];
  expect(toast?.querySelector('[data-slot="alert-title"]')?.textContent).toBe("cpd_failed");
  expect(toast?.querySelector('[data-slot="alert-description"]')?.textContent).toBe(refusal);
  expect(dialog?.querySelector('[data-part="settings-error"]')).toBeNull();
});

it("the Agent Tiers Save shows a refused concurrency as the error toast with the whole message", () => {
  // The CLI's whole refusal text, uncut: the toast must carry every line, not a first line or a truncated one.
  const refusal = "concurrency must be an integer from 1 to 16 inclusive\n  key: concurrency\n  value: 99";
  const readWithConcurrency = { ...read, concurrency: 3 } as SettingsRead;
  const transport = new FakeTransport();
  const reader = { readBase64: async () => "", readText: async () => "" };
  mounted = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => "c1"} />);
  act(() => transport.emitStatus("open"));
  click(body.querySelector('[data-part="settings"]'));
  act(() => transport.emitMessage({ type: "settings.result", settings: readWithConcurrency }));
  const dialog = body.querySelector('[data-part="agent-tiers-dialog"]');
  expect(dialog).not.toBeNull();
  // The Worker count is a numeric field of the Models + Tiers section (web/CONTRACT.md §19.1, commands.md §14.2c).
  const input = body.querySelector('[aria-label="concurrency"]') as HTMLInputElement | null;
  expect(input).not.toBeNull();
  if (input === null) throw new Error("the Agent Tiers dialog must show the Worker concurrency");
  act(() => { input.value = "99"; input.dispatchEvent(new Event("input", { bubbles: true })); });
  click(Array.from(dialog?.querySelectorAll("button") ?? []).find((b) => b.textContent === "Save") ?? null);
  expect(transport.sent).toContainEqual({ type: "settings.save", changes: { concurrency: 99 } });
  const sentBefore = transport.sent.length;
  act(() => transport.emitMessage({ type: "settings.saved", ok: false, error: { code: "config_invalid", message: refusal } }));
  const toasts = body.querySelectorAll('[data-part="cockpit-error-toast"]');
  expect(toasts).toHaveLength(1);
  const toast = toasts[0];
  expect(toast?.querySelector('[data-slot="alert-title"]')?.textContent).toBe("config_invalid");
  expect(toast?.querySelector('[data-slot="alert-description"]')?.textContent).toBe(refusal);
  // Never inline, and a refused save stops the remaining writes: nothing more is sent and the dialog stays open.
  expect(dialog?.querySelector('[data-part="settings-error"]')).toBeNull();
  expect(transport.sent.length).toBe(sentBefore);
  expect(body.querySelector('[data-part="agent-tiers-dialog"]')).not.toBeNull();
});

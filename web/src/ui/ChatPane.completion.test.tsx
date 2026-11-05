// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import type { Completion, SessionRef } from "../protocol/domain.ts";
import type { Connection } from "../state/connection.ts";
import { initialState, reduce, type SessionView } from "../state/store.ts";
import { mount } from "../testing/render.tsx";
import { ChatPane } from "./ChatPane.tsx";

vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

const completion: Completion = { label: "safe_to_archive", text: "Safe to archive · 93%", score: 93, tone: "done" };
const reader = { readBase64: async () => "QUJD", readText: async () => "text" };
const baseView = (): SessionView => {
  const s = reduce(initialState, { type: "selectSession", sessionId: "s1" }).session;
  if (s === null) throw new Error("no session");
  return s;
};
const viewOf = (role: SessionRef["role"], withCompletion: Completion | null): SessionView => ({
  ...baseView(),
  ref: { id: "s1", key: "ctx:plan", runner: "durable", role, state: "idle", title: "My chat" },
  title: "My chat",
  completion: withCompletion,
});
const connection = new Proxy({}, { get: () => () => undefined }) as unknown as Connection;

it("ChatPane shows the completion badge of a Lead session", () => {
  const m = mount(<ChatPane view={viewOf("lead", completion)} connection={connection} reader={reader} />);
  const badge = m.container.querySelector('[data-part="completion-badge"]');
  expect(badge?.textContent).toBe("Safe to archive · 93%");
  m.unmount();
  const mgr = mount(<ChatPane view={viewOf("manager", completion)} connection={connection} reader={reader} />);
  expect(mgr.container.querySelector('[data-part="completion-badge"]')).toBeNull();
  mgr.unmount();
});

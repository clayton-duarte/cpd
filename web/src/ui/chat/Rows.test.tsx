// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AssistantItem, ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import "../../testing/render.tsx";
import { Transcript } from "./Transcript.tsx";

const calls: string[] = [];
vi.mock("react-markdown", () => ({
  default: (props: { children: string }) => {
    calls.push(props.children);
    return <p>{props.children}</p>;
  },
}));
vi.mock("remark-gfm", () => ({ default: () => undefined }));

const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };
const asst = (id: string, text: string): AssistantItem => ({ kind: "assistant", id, streaming: false, blocks: [{ type: "text", text }] });

let cleanup: (() => void) | null = null;
beforeEach(() => { calls.length = 0; });
afterEach(() => { cleanup?.(); cleanup = null; });

function setup(items: readonly TranscriptItem[]) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  cleanup = () => { act(() => root.unmount()); container.remove(); };
  const render = (next: readonly TranscriptItem[]) => act(() => root.render(<Transcript {...base} items={next} />));
  render(items);
  return { container, render };
}

it("an update to one assistant item does not re-render the markdown of other items", () => {
  const a1 = asst("a1", "alpha text");
  const a2 = asst("a2", "beta text");
  const a3 = asst("a3", "gamma text");
  const { render } = setup([a1, a2, a3]);
  expect([...calls].sort()).toEqual(["alpha text", "beta text", "gamma text"]);
  calls.length = 0;
  render([a1, asst("a2", "beta text longer"), a3]);
  expect(calls).toEqual(["beta text longer"]);
});

it("a chain row does not re-render when an unrelated item changes", () => {
  let reads = 0;
  const args = new Proxy({ path: "/x" }, { get(t, k, r) { reads++; return Reflect.get(t, k, r); } });
  const mk = (n: number): ToolItem => ({ kind: "tool", id: `tool:${n}`, callId: String(n), name: "read", args, status: "running" });
  const t1 = mk(1);
  const t2 = mk(2);
  const a1 = asst("a1", "alpha");
  const { container, render } = setup([a1, t1, t2]);
  act(() => container.querySelector<HTMLElement>('[data-row^="chain:"] [data-slot="trace-collapsible-trigger"]')!.click());
  const row = container.querySelector('[data-row^="chain:"]');
  expect(row).not.toBeNull();
  const firstChild = row!.firstElementChild;
  expect(reads).toBeGreaterThan(0);
  reads = 0;
  render([asst("a1", "alpha changed"), t1, t2]);
  expect(reads).toBe(0);
  expect(container.querySelector('[data-row^="chain:"]')).toBe(row);
  expect(row!.firstElementChild).toBe(firstChild);
});

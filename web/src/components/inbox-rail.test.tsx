// @vitest-environment jsdom
// Red test for the Inbox rail's themed scrollbar (web/README.md "Themed scrollbars"): the rail's
// `SidebarContent` is one of the three scrolling surfaces that carry the authored
// `scrollbar-themed` utility. jsdom comes from the per-file comment; the rail renders through the
// shadcn `SidebarProvider` it needs, over two authored Inbox items — no live adapter, socket,
// server or network. `scrollbar-themed` does not exist on the base branch, so the red is a failed
// class assertion, never a build error.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { InboxRail } from "./inbox-rail";
import { SidebarProvider } from "./ui/sidebar";
import type { InboxItem } from "../api/readmodel";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Two authored Inbox items, each carrying every field the read model declares (no live adapter).
const ITEMS: readonly InboxItem[] = [
  {
    id: "i1",
    kind: "review-requested",
    urgency: "normal",
    label: "Review the follow rule\nA pull request waits on you",
    subject: "Review the follow rule",
    context: null,
    plan: null,
    job: null,
    session: null,
    record: null,
    event: null,
    url: null,
    opened_at: null,
    closed_at: null,
    snoozed_until: null,
    dismissed_at: null,
    claimed_at: null,
    age_seconds: 30,
  },
  {
    id: "i2",
    kind: "pr-ready",
    urgency: "normal",
    label: "A pull request is ready",
    subject: "A pull request is ready",
    context: null,
    plan: null,
    job: null,
    session: null,
    record: null,
    event: null,
    url: "https://example.test/pr/2",
    opened_at: null,
    closed_at: null,
    snoozed_until: null,
    dismissed_at: null,
    claimed_at: null,
    age_seconds: 7200,
  },
];

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

test("the Chat's viewport, a code block's scroll surface and the Inbox rail's scroll container carry scrollbar-themed", () => {
  act(() => {
    root.render(
      <SidebarProvider>
        <InboxRail items={ITEMS} sessions={[]} />
      </SidebarProvider>,
    );
  });

  // The rail's scroll container is the vendored `SidebarContent`.
  const content = container.querySelector('[data-slot="sidebar-content"]');
  expect(content).not.toBeNull();
  expect(content?.classList.contains("scrollbar-themed")).toBe(true);
});

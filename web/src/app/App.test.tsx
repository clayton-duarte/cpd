// @vitest-environment jsdom
// Red tests for the resizable three-column shell (web/README.md "The resizable three-column
// shell", hs-d824): Context | Chat | Inbox. The shell replaces the single-screen App, so every
// test renders `App` exactly as `main.tsx` mounts it and pins what the screen shows. The impl
// Job owns `App.tsx`, the vendored `sidebar-03`/`resizable` components and `web/index.html`;
// these tests fail on the current tree and pass once it lands.
//
// Only authored code is tested. The LiveApi is an injected fake; no EventSource opens, no fetch
// runs, no socket, server or cpd spawns. jsdom has no layout and no ResizeObserver, so the tests
// fake the group's measurement (a 1600 px rail of three 1600/3 px panels) and let
// react-resizable-panels compute the real percentages; the panel's `flex` style then carries the
// Context column's width, which is what the size and double-click tests read.
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { App } from "./App";
import type { LiveApi } from "../api/live";
import type { LiveSession } from "../api/live-server";
import type { InboxItem } from "../api/readmodel";
import type { SnapshotFrame } from "../fold/fold";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** The Context column's default width (web/README.md "Sizing") and the CSS root font size. */
const CONTEXT_DEFAULT_REM = 20;
const REM_PX = 16;
/** The faked group width, and the panel/handle widths the fake gives its children. */
const GROUP_PX = 1600;
const PANEL_PX = GROUP_PX / 3;
const HANDLE_PX = 2;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  // shadcn's `use-mobile` hook reads `window.matchMedia`; jsdom has none, and the desktop rail
  // (not the mobile sheet) is what the Context column renders.
  window.matchMedia = (() => ({
    matches: false,
    media: "",
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent() {
      return false;
    },
  })) as unknown as typeof window.matchMedia;

  // jsdom lays nothing out: report positions so react-resizable-panels computes real sizes.
  // Panels and handles are the group's children, so each one's left is the sum before it.
  Element.prototype.getBoundingClientRect = function (this: Element): DOMRect {
    let left = 0;
    const parent = this.parentElement;
    if (parent !== null) {
      for (const child of Array.from(parent.children)) {
        if (child === this) break;
        left += child.hasAttribute("data-panel") ? PANEL_PX : HANDLE_PX;
      }
    }
    const width = this.hasAttribute("data-panel") ? PANEL_PX : HANDLE_PX;
    return {
      width,
      height: 900,
      top: 0,
      left,
      right: left + width,
      bottom: 900,
      x: left,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect;
  };
  // The group's width is the sum of its panels' `offsetWidth`s (the library measures each axis
  // that way when it has no layout strategy); fake the group too so the sum is GROUP_PX.
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.hasAttribute("data-group")) return GROUP_PX;
      if (this.hasAttribute("data-panel")) return PANEL_PX;
      return HANDLE_PX;
    },
  });

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

/** Flushes pending microtasks (the shell's mount effects). */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
}

/** Renders the shell the way `main.tsx` mounts it, with no live adapter. */
async function renderShell(): Promise<void> {
  await act(async () => {
    root.render(<App />);
    await flush();
  });
}

/** Renders the shell with an injected live adapter: no network, no EventSource, no socket. */
async function renderLive(api: LiveApi): Promise<void> {
  await act(async () => {
    root.render(<App token="t0k" live={api} />);
    await flush();
  });
}

/** The three panels in document order: Context, Chat, Inbox. */
function columns(): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>("[data-panel]")];
}

/** The panel's computed share of the group, from the library's inline `flex` shorthand. */
function columnWidth(el: HTMLElement): number {
  return Number(/flex:\s*([\d.]+)/.exec(el.getAttribute("style") ?? "")?.[1]);
}

/** Drags the boundary from one x to another through the library's pointer handlers. */
function drag(fromX: number, toX: number): void {
  const base = {
    bubbles: true,
    cancelable: true,
    clientY: 100,
    pointerId: 1,
    pointerType: "mouse",
    buttons: 1,
  } as const;
  act(() => {
    document.dispatchEvent(new PointerEvent("pointerdown", { ...base, clientX: fromX }));
    document.dispatchEvent(new PointerEvent("pointermove", { ...base, clientX: toX }));
    document.dispatchEvent(new PointerEvent("pointerup", { ...base, clientX: toX, buttons: 0 }));
  });
}

test("the shell renders three resizable columns", async () => {
  await renderShell();

  // One horizontal panel group holding Context, Chat and Inbox, with a handle between each.
  expect(container.querySelector("[data-group]")).not.toBeNull();
  expect(columns()).toHaveLength(3);
  expect(container.querySelectorAll("[data-separator]")).toHaveLength(2);

  // The centre column is the existing Live Chat; with no live adapter it shows its explicit
  // unavailable state (web/README.md "The chat surface (shadcn AI Elements)").
  const chat = columns()[1]!;
  expect(chat.textContent).toContain("Live adapter off");
});

test("the Context column renders the sidebar-03 block", async () => {
  await renderShell();

  const columns_ = columns();
  expect(columns_).toHaveLength(3);
  // The shadcn `sidebar` block's root marker; it is the Context column and nothing else.
  expect(columns_[0]!.querySelector('[data-slot="sidebar"]')).not.toBeNull();
});

/** The Inbox rail: the shell's right column. */
function inboxRail(): HTMLElement {
  const columns_ = columns();
  expect(columns_).toHaveLength(3);
  return columns_[2]!;
}

test("the Inbox rail's header carries the title and no search input, and no Unreads switch", async () => {
  await renderShell();

  const rail = inboxRail();
  // The header is the title alone: the `Inbox` element, not the placeholder's sentence.
  const title = [...rail.querySelectorAll<HTMLElement>("*")].find(
    (el) => el.textContent?.trim() === "Inbox",
  );
  expect(title).toBeDefined();

  // The block's `SidebarInput` (`Type to search...`) is deliberately not vendored: no input at all.
  expect(rail.querySelectorAll("input")).toHaveLength(0);
  expect(rail.querySelector('[data-slot="sidebar-input"]')).toBeNull();

  // The block's `Unreads` switch is deliberately not vendored.
  expect(rail.textContent).not.toContain("Unreads");
});

test("the Inbox rail has no icon column", async () => {
  const { api } = fakeLive([], [inboxItem({ label: "Curate the docs", age_seconds: 5 })]);
  await renderLive(api);

  const rail = inboxRail();
  // The rail is the block's second sidebar (its mail list), not the placeholder.
  expect(rail.querySelector('[data-slot="sidebar"]')).not.toBeNull();
  expect(rail.textContent).toContain("Curate the docs");
  // The block's first sidebar — its icon column (Drafts, Sent, Junk, Trash) — is not vendored.
  for (const label of ["Drafts", "Sent", "Junk", "Trash"]) {
    expect(rail.textContent).not.toContain(label);
  }
});

test("the Context rail cannot be collapsed: it renders no rail toggle", async () => {
  await renderShell();

  // The Context column and its rail are still there...
  const columns_ = columns();
  expect(columns_).toHaveLength(3);
  expect(columns_[0]!.querySelector('[data-slot="sidebar"]')).not.toBeNull();
  // ...but sidebar-03's `SidebarRail` — the 4px strip that collapsed the rail on a stray click —
  // is not, in either rail.
  expect(container.querySelector('[data-slot="sidebar-rail"]')).toBeNull();
  expect(container.querySelector('[aria-label="Toggle Sidebar"]')).toBeNull();
});

test("the Context column defaults to 20rem and a double-click restores it", async () => {
  await renderShell();

  const columns_ = columns();
  expect(columns_).toHaveLength(3);
  const context = columns_[0]!;
  const handle = container.querySelector<HTMLElement>("[data-separator]");
  expect(handle).not.toBeNull();

  // 20rem of the faked 1600 px group: the mount effect re-applies the ref's exactly-20rem size.
  const expected = ((CONTEXT_DEFAULT_REM * REM_PX) / GROUP_PX) * 100;
  expect(columnWidth(context)).toBeCloseTo(expected, 2);

  // Dragging the handle right grows the Context column away from its default.
  drag(PANEL_PX + HANDLE_PX / 2, PANEL_PX + HANDLE_PX / 2 + 80);
  expect(columnWidth(context)).not.toBeCloseTo(expected, 2);

  // Double-clicking the handle restores the 20rem default.
  await act(async () => {
    handle!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true }));
    await flush();
  });
  expect(columnWidth(context)).toBeCloseTo(expected, 2);
});

/** A Session as the tree and Chat now see it: `LiveSession` plus `context` and `name`. */
type TreeSession = LiveSession & { context: string; name?: string };

/**
 * A fake LiveApi: the sessions and live Inbox items it returns, the ids App subscribed to, and
 * the frame emitter. `inbox()` is the Inbox rail's read (web/README.md "The live Inbox in the
 * right rail"); the tests feed items through it and never run `cpd inbox list`.
 */
function fakeLive(
  sessions: readonly TreeSession[],
  inbox: readonly InboxItem[] = [],
): {
  api: LiveApi;
  subscribed: string[];
  emit: (frame: SnapshotFrame) => void;
} {
  const subscribed: string[] = [];
  let onFrame: ((frame: SnapshotFrame) => void) | undefined;
  const api = {
    sessions: async () => sessions,
    inbox: async () => inbox,
    subscribe: (id: string, callback: (frame: SnapshotFrame) => void) => {
      subscribed.push(id);
      onFrame = callback;
      return () => {
        onFrame = undefined;
      };
    },
    send: async () => {},
  } as unknown as LiveApi;
  return { api, subscribed, emit: (frame) => onFrame?.(frame) };
}

/**
 * One live Inbox item as `cpd inbox list` prints it (web/README.md "The live Inbox in the right
 * rail"): the CLI row's fields, with the ones the rail does not render left at their default.
 */
function inboxItem(overrides: Partial<InboxItem>): InboxItem {
  return {
    id: "item-1",
    kind: "pr-ready",
    urgency: "blue",
    label: "Ready",
    subject: "",
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
    age_seconds: 0,
    ...overrides,
  };
}

test("picking a lead in the rail shows that lead's transcript and leaves no picker", async () => {
  const manager: TreeSession = {
    id: "manager-1",
    role: "manager",
    runner: "durable",
    context: "ctx",
    name: "Manager One",
  };
  const lead: TreeSession = {
    id: "lead-1",
    role: "lead",
    runner: "durable",
    context: "ctx",
    name: "Lead One",
  };
  const { api, subscribed, emit } = fakeLive([manager, lead]);

  await act(async () => {
    root.render(<App token="t0k" live={api} />);
    await flush();
  });

  // The rail carries the lead's row; App grabs the Session it names.
  const leadRow = container.querySelector<HTMLButtonElement>('button[data-session-id="lead-1"]');
  expect(leadRow).not.toBeNull();
  if (leadRow === null) return;

  await act(async () => {
    leadRow.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await flush();
  });

  // The rail is the Chat's only navigation: picking the lead subscribes the Chat to that lead.
  expect(subscribed).toEqual(["lead-1"]);

  await act(async () => {
    emit({
      type: "snapshot",
      epoch: "live",
      seq: 1,
      snapshot: {
        messages: [{ id: "m1", role: "assistant", text: "the lead transcript", pending: false }],
      },
    });
    await flush();
  });
  expect(container.textContent).toContain("the lead transcript");
  // The old Session picker left the Chat.
  expect(container.querySelector("select")).toBeNull();
});

test("the document defaults to the dark theme", () => {
  // Dark is the default: web/index.html's `<html>` element carries `class="dark"`.
  const html = readFileSync(`${import.meta.dirname}/../../index.html`, "utf8");
  const rootTag = /<html[^>]*>/i.exec(html)?.[0] ?? "";
  expect(rootTag).toMatch(/\bclass="[^"]*\bdark\b[^"]*"/);
});

/** The Inbox rail's rows, one anchor per live item (web/README.md "The live Inbox"). */
function inboxRows(): HTMLElement[] {
  return [...inboxRail().querySelectorAll<HTMLElement>("a")];
}

/** The name of a row: the first span of the row's top line, before the pushed-right date. */
function rowName(row: Element): string | null {
  return row.querySelector("div > span")?.textContent ?? null;
}

/** The date of a row: the span pushed right by the block's `ml-auto`. */
function rowDate(row: Element): string | null {
  return row.querySelector(".ml-auto")?.textContent ?? null;
}

test("the right rail renders the live Inbox items, not the sample mails", async () => {
  const item = inboxItem({
    kind: "review-requested",
    label: "Review requested\nacme/api#812",
    url: "https://example.test/acme/api/pull/812",
    age_seconds: 30,
  });
  const { api } = fakeLive([], [item]);

  await renderLive(api);

  // The real item's label leads the rail...
  const rail = inboxRail();
  expect(rail.textContent).toContain("Review requested");
  expect(rail.textContent).toContain("acme/api#812");
  // ...and the block's ten registry sample mails are gone with this PR.
  expect(rail.textContent).not.toContain("William Smith");
  expect(rail.textContent).not.toContain("Sophia White");
});

test("an Inbox row's name is its actor: peer review, the Session's name, or cpd engine", async () => {
  const session: TreeSession = {
    id: "sess-abcdef123456",
    role: "lead",
    runner: "durable",
    context: "ctx",
    name: "Tree Lead",
  };
  const items = [
    inboxItem({ id: "peer-1", kind: "review-requested", label: "Review requested", session: session.id }),
    inboxItem({ id: "session-1", kind: "pr-ready", label: "Ready", session: session.id }),
    inboxItem({ id: "engine-1", kind: "pr-ready", label: "Ready", session: null }),
  ];
  const { api } = fakeLive([session], items);

  await renderLive(api);

  expect(inboxRows().map((row) => rowName(row))).toEqual(["peer review", "Tree Lead", "cpd engine"]);
});

test("an Inbox row's date is its age rendered relative", async () => {
  const ages: readonly (readonly [number, string])[] = [
    [30, "just now"],
    [300, "5 minutes ago"],
    [10_800, "3 hours ago"],
    [108_000, "Yesterday"],
    [259_200, "3 days ago"],
    [1_209_600, "2 weeks ago"],
    [6_480_000, "2 months ago"],
  ];
  const items = ages.map(([age_seconds], index) =>
    inboxItem({ id: `item-${index}`, label: `Item ${index}`, age_seconds }),
  );
  const { api } = fakeLive([], items);

  await renderLive(api);

  expect(inboxRows().map((row) => rowDate(row))).toEqual(ages.map(([, rendered]) => rendered));
});

test("an Inbox row shows the label's first line as its subject and its second line as its teaser only when there is one", async () => {
  const twoLine = inboxItem({
    id: "two-line",
    kind: "pr-ready",
    label: "Ready to merge\nacme/api#7",
  });
  const oneLine = inboxItem({ id: "one-line", kind: "curate-docs", label: "Curate the docs" });
  const { api } = fakeLive([], [twoLine, oneLine]);

  await renderLive(api);

  const [twoLineRow, oneLineRow] = inboxRows();
  expect(twoLineRow!.querySelector(".font-medium")?.textContent).toBe("Ready to merge");
  expect(twoLineRow!.querySelector(".line-clamp-2")?.textContent).toBe("acme/api#7");
  expect(oneLineRow!.querySelector(".font-medium")?.textContent).toBe("Curate the docs");
  expect(oneLineRow!.querySelector(".line-clamp-2")).toBeNull();
});

test("an Inbox row links to its item's url, and to # when it has none", async () => {
  const linked = inboxItem({
    id: "linked",
    kind: "pr-ready",
    label: "Ready",
    url: "https://example.test/acme/api/pull/7",
  });
  const unlinked = inboxItem({ id: "unlinked", kind: "curate-docs", label: "Curate the docs", url: null });
  const { api } = fakeLive([], [linked, unlinked]);

  await renderLive(api);

  const [linkedRow, unlinkedRow] = inboxRows();
  expect(linkedRow!.getAttribute("href")).toBe("https://example.test/acme/api/pull/7");
  expect(unlinkedRow!.getAttribute("href")).toBe("#");
});

test("an empty Inbox renders an empty list, with no invented copy", async () => {
  const { api } = fakeLive([], []);

  await renderLive(api);

  const rail = inboxRail();
  expect(rail.querySelectorAll("a")).toHaveLength(0);
  // Only the header's title; the body adds no placeholder sentence.
  expect(rail.textContent?.trim()).toBe("Inbox");
});

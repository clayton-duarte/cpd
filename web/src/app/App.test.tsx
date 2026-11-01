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

test("the Inbox column renders its placeholder", async () => {
  await renderShell();

  const columns_ = columns();
  expect(columns_).toHaveLength(3);
  expect(columns_[2]!.textContent).toContain("The Inbox lands here.");
});

/**
 * sidebar-09's registry sample data (`data.mails`), verbatim and in its order
 * (web/README.md "The Inbox rail in the right column"): the ten mails the rail lists, each with
 * the name, the date pushed right, the subject and the two-line teaser. Rows and classes are the
 * block's, so the tests read the rendered text and the block's row classes, never a source file.
 */
const INBOX_MAILS = [
  {
    name: "William Smith",
    date: "09:34 AM",
    subject: "Meeting Tomorrow",
    teaser:
      "Hi team, just a reminder about our meeting tomorrow at 10 AM.\nPlease come prepared with your project updates.",
  },
  {
    name: "Alice Smith",
    date: "Yesterday",
    subject: "Re: Project Update",
    teaser:
      "Thanks for the update. The progress looks great so far.\nLet's schedule a call to discuss the next steps.",
  },
  {
    name: "Bob Johnson",
    date: "2 days ago",
    subject: "Weekend Plans",
    teaser:
      "Hey everyone! I'm thinking of organizing a team outing this weekend.\nWould you be interested in a hiking trip or a beach day?",
  },
  {
    name: "Emily Davis",
    date: "2 days ago",
    subject: "Re: Question about Budget",
    teaser:
      "I've reviewed the budget numbers you sent over.\nCan we set up a quick call to discuss some potential adjustments?",
  },
  {
    name: "Michael Wilson",
    date: "1 week ago",
    subject: "Important Announcement",
    teaser:
      "Please join us for an all-hands meeting this Friday at 3 PM.\nWe have some exciting news to share about the company's future.",
  },
  {
    name: "Sarah Brown",
    date: "1 week ago",
    subject: "Re: Feedback on Proposal",
    teaser:
      "Thank you for sending over the proposal. I've reviewed it and have some thoughts.\nCould we schedule a meeting to discuss my feedback in detail?",
  },
  {
    name: "David Lee",
    date: "1 week ago",
    subject: "New Project Idea",
    teaser:
      "I've been brainstorming and came up with an interesting project concept.\nDo you have time this week to discuss its potential impact and feasibility?",
  },
  {
    name: "Olivia Wilson",
    date: "1 week ago",
    subject: "Vacation Plans",
    teaser:
      "Just a heads up that I'll be taking a two-week vacation next month.\nI'll make sure all my projects are up to date before I leave.",
  },
  {
    name: "James Martin",
    date: "1 week ago",
    subject: "Re: Conference Registration",
    teaser:
      "I've completed the registration for the upcoming tech conference.\nLet me know if you need any additional information from my end.",
  },
  {
    name: "Sophia White",
    date: "1 week ago",
    subject: "Team Dinner",
    teaser:
      "To celebrate our recent project success, I'd like to organize a team dinner.\nAre you available next Friday evening? Please let me know your preferences.",
  },
] as const;

/** The Inbox rail: the shell's right column. */
function inboxRail(): HTMLElement {
  const columns_ = columns();
  expect(columns_).toHaveLength(3);
  return columns_[2]!;
}

test("the right column lists the Inbox rail's ten mails with their name, date, subject and teaser", async () => {
  await renderShell();

  // The block's mail list: one anchor per mail, rows and classes verbatim (web/README.md).
  const rows = [...inboxRail().querySelectorAll("a")];
  expect(rows).toHaveLength(INBOX_MAILS.length);

  INBOX_MAILS.forEach((mail, index) => {
    const row = rows[index]!;
    // The name leads the row; the date is pushed right (`ml-auto`), the subject is `font-medium`,
    // and the teaser is the two-line `line-clamp-2` span (its two lines separated by a newline).
    expect(row.textContent).toContain(mail.name);
    expect(row.querySelector(".ml-auto")?.textContent).toBe(mail.date);
    expect(row.querySelector(".font-medium")?.textContent).toBe(mail.subject);
    const teaser = row.querySelector(".line-clamp-2");
    expect(teaser?.textContent).toBe(mail.teaser);
    expect(mail.teaser.split("\n")).toHaveLength(2);
  });
});

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
  await renderShell();

  const rail = inboxRail();
  // The rail is the block's second sidebar (its mail list), not the placeholder.
  expect(rail.textContent).toContain("William Smith");
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

/** A fake LiveApi: the sessions it lists, the ids App subscribed to, and the frame emitter. */
function fakeLive(sessions: readonly TreeSession[]): {
  api: LiveApi;
  subscribed: string[];
  emit: (frame: SnapshotFrame) => void;
} {
  const subscribed: string[] = [];
  let onFrame: ((frame: SnapshotFrame) => void) | undefined;
  const api: LiveApi = {
    sessions: async () => sessions,
    subscribe: (id, callback) => {
      subscribed.push(id);
      onFrame = callback;
      return () => {
        onFrame = undefined;
      };
    },
    send: async () => {},
  };
  return { api, subscribed, emit: (frame) => onFrame?.(frame) };
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

// @vitest-environment jsdom
// Red tests for the Context tree in the left rail (web/README.md "The Context tree in the left
// rail selects the Chat"): the live Sessions grouped by Context, each Context's manager at the
// root with that Context's leads nested beneath it, every row a <button> carrying its Session's
// id, and the selected row marked active. The tree props (`sessions`, `selectedId`,
// `onSelectSession`) do not exist on the base branch, so the component is reached through a
// narrow cast and the red is a failed assertion, never a build error.
//
// Only authored code is tested: the Sessions are plain data, no LiveApi, socket, network or
// server. jsdom comes from the per-file comment; `use-mobile` reads `window.matchMedia`, which
// jsdom lacks, so the desktop rail (not the mobile sheet) is faked.
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { AppSidebar } from "./app-sidebar";
import { SidebarProvider } from "./ui/sidebar";
import type { LiveSession } from "../api/live-server";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** A Session as the tree sees it: `LiveSession` plus the adapter's new `context` and `name`. */
type TreeSession = LiveSession & { context: string; name?: string };

/** The tree's props; AppSidebar does not declare them on the base branch, so cast to reach them. */
interface TreeProps {
  sessions: readonly TreeSession[];
  selectedId?: string;
  onSelectSession: (id: string) => void;
}

const Tree = AppSidebar as unknown as (props: TreeProps) => ReactElement;

// Two Contexts (`alpha`, `beta`) with ids long enough to show their first-8-character prefix.
const MA = "aaaaaaaa-0000-0000-0000-000000000001";
const MB = "bbbbbbbb-0000-0000-0000-000000000001";
const LA1 = "aaaaaaaa-0000-0000-0000-000000000002";
const LA2 = "aaaaaaaa-0000-0000-0000-000000000003";

// Listed beta-first to prove the contexts sort by name (`alpha` before `beta`).
const SESSIONS: readonly TreeSession[] = [
  { id: MB, role: "manager", runner: "durable", context: "beta", name: "Manager Beta" },
  { id: LA2, role: "lead", runner: "durable", context: "alpha", name: "Lead Alpha Two" },
  { id: MA, role: "manager", runner: "durable", context: "alpha", name: "Manager Alpha" },
  { id: LA1, role: "lead", runner: "durable", context: "alpha", name: "Lead Alpha One" },
];

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
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

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

/** Renders the tree inside the provider App mounts it in. */
function render(
  sessions: readonly TreeSession[],
  selectedId?: string,
  onSelectSession: (id: string) => void = () => {},
): void {
  act(() => {
    root.render(
      <SidebarProvider>
        <Tree sessions={sessions} selectedId={selectedId} onSelectSession={onSelectSession} />
      </SidebarProvider>,
    );
  });
}

/** Every row: a real <button> carrying its Session's id. */
function rows(): HTMLButtonElement[] {
  return [...container.querySelectorAll<HTMLButtonElement>("button[data-session-id]")];
}

function row(id: string): HTMLButtonElement | undefined {
  return rows().find((button) => button.getAttribute("data-session-id") === id);
}

function ids(buttons: readonly HTMLElement[]): string[] {
  return buttons.map((button) => button.getAttribute("data-session-id") ?? "");
}

/** A nested row lives inside a `SidebarMenuSub`; a root row does not. */
function nested(button: HTMLElement): boolean {
  return button.closest('[data-slot="sidebar-menu-sub"]') !== null;
}

test("the tree shows one root row per Context with that Context's leads nested beneath it", () => {
  render(SESSIONS);

  // One row per Session, nothing dropped.
  expect(new Set(ids(rows()))).toEqual(new Set([MA, MB, LA1, LA2]));

  // The managers are the roots, in Context-name order (`alpha` before `beta`).
  const roots = rows().filter((button) => !nested(button));
  expect(ids(roots)).toEqual([MA, MB]);

  // Each Context's leads nest beneath that Context's manager.
  const managerAlpha = row(MA);
  const leadAlpha = row(LA1);
  expect(managerAlpha).toBeDefined();
  expect(leadAlpha).toBeDefined();
  if (managerAlpha !== undefined && leadAlpha !== undefined) {
    expect(nested(leadAlpha)).toBe(true);
    const group = managerAlpha.closest('[data-slot="sidebar-menu-item"]');
    expect(group).not.toBeNull();
    expect(group?.contains(leadAlpha) ?? false).toBe(true);
    expect(group?.contains(row(LA2) ?? null) ?? false).toBe(true);
  }
});

test("every row carries its Session id and clicking one reports it", () => {
  const clicked: string[] = [];
  render(SESSIONS, undefined, (id) => clicked.push(id));

  // Every Session is a row.
  expect(rows()).toHaveLength(SESSIONS.length);

  for (const session of SESSIONS) {
    const button = row(session.id);
    expect(button).toBeDefined();
    if (button === undefined) continue;
    // `data-session-id` and `title` carry the full uuid; the id's first 8 characters show muted.
    expect(button.getAttribute("title")).toBe(session.id);
    expect(button.textContent ?? "").toContain(session.id.slice(0, 8));
  }

  const lead = row(LA1);
  expect(lead).toBeDefined();
  if (lead === undefined) return;
  act(() => {
    lead.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  expect(clicked).toEqual([LA1]);
});

test("the header reads cpd with the Context count, never Documentation", () => {
  const header = (): string =>
    container.querySelector('[data-slot="sidebar-header"]')?.textContent ?? "";

  // Four Sessions across two Contexts: `2 contexts`.
  render(SESSIONS);
  expect(header()).toContain("cpd");
  expect(header()).toContain("2 contexts");

  // One Context: the singular.
  render([SESSIONS[0]]);
  expect(header()).toContain("1 context");

  // No Sessions: the empty state's own pluralisation.
  render([]);
  expect(header()).toContain("No contexts");

  // The `sidebar-03` block's sample header text is gone from the rail.
  expect(container.textContent ?? "").not.toContain("Documentation");
  expect(container.textContent ?? "").not.toContain("v1.0.0");
});

test("the row of the selected Session is marked active", () => {
  render(SESSIONS, LA1);

  const selected = row(LA1);
  const other = row(MA);
  expect(selected).toBeDefined();
  expect(other).toBeDefined();
  if (selected === undefined || other === undefined) return;
  // `isActive` on the selected row's sidebar button renders as `data-active`.
  expect(selected.getAttribute("data-active")).toBe("true");
  expect(other.getAttribute("data-active")).not.toBe("true");
});

// @vitest-environment jsdom
// Red tests for the Context tree in the left rail (web/README.md "The Context tree in the left
// rail selects the Chat"): the live Sessions grouped by Context, each Context's manager at the
// root with that Context's leads nested beneath it, every row a <button> carrying its Session's
// id, and the selected row marked active.
//
// The corrected Context tree (web/README.md "Context tree rows"): the rail lists EVERY Context
// of the Contexts read, an archived Context starts folded, a Context row selects its manager
// Session and is disabled only without one, a Context's archived Plans hide behind its
// `aria-expanded` fold, and the rows' dots are the palette's mizu / plain values, never a
// Tailwind hue. The tree props (`sessions`, `plans`, `contexts`, `selectedId`,
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
import type { LiveContext, LivePlan, LiveSession } from "../api/live-server";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** A Session as the tree sees it: `LiveSession` plus the adapter's new `context` and `name`. */
type TreeSession = LiveSession & { context: string; name?: string };

/** The tree's props; AppSidebar does not declare them on the base branch, so cast to reach them. */
interface TreeProps {
  sessions: readonly TreeSession[];
  plans?: readonly LivePlan[];
  contexts?: readonly LiveContext[];
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

// The Plan tree: one Context (`building-cpd`) with two Plans, listed beta-first so the Context /
// Plan-key sort is exercised. `alpha` carries nonzero counts, `beta` every count zero.
const CONTEXT = "building-cpd";
const LEAD_A = "aaaaaaaa-1111-1111-1111-111111111111";
const LEAD_B = "bbbbbbbb-2222-2222-2222-222222222222";

const PLAN_ALPHA: LivePlan = {
  id: "alpha",
  context: CONTEXT,
  title: "Alpha plan",
  phase: "dispatch",
  lead_session: LEAD_A,
  archived_at: null,
  counts: { draft: 1, queued: 2, running: 1, merged: 4, failed: 0, cancelled: 0 },
};
const PLAN_BETA: LivePlan = {
  id: "beta",
  context: CONTEXT,
  title: "Beta plan",
  phase: "plan",
  lead_session: LEAD_B,
  archived_at: null,
  counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
};
const PLANS: readonly LivePlan[] = [PLAN_BETA, PLAN_ALPHA];

// The Contexts read for the Plan tree: every Context the Plans hang beneath, none archived.
const CONTEXTS: readonly LiveContext[] = [{ id: CONTEXT, archived: false }];

// The Plans' Lead Sessions, so the rail can draw each Plan row's glyph from a real state dot.
const PLAN_SESSIONS: readonly TreeSession[] = [
  { id: LEAD_A, role: "lead", runner: "durable", context: CONTEXT, name: "Lead A", state: "working" },
  { id: LEAD_B, role: "lead", runner: "durable", context: CONTEXT, name: "Lead B", state: "idle" },
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

/** Renders the tree with the Plans read wired, so the rail draws its Plan tree. */
function renderPlans(
  plans: readonly LivePlan[],
  sessions: readonly TreeSession[] = PLAN_SESSIONS,
  onSelectSession: (id: string) => void = () => {},
  contexts?: readonly LiveContext[],
): void {
  act(() => {
    root.render(
      <SidebarProvider>
        <Tree sessions={sessions} plans={plans} contexts={contexts} onSelectSession={onSelectSession} />
      </SidebarProvider>,
    );
  });
}

/**
 * Renders the corrected Context tree: the Contexts read (`contexts`) is the row source, the Plans
 * read (`plans`) hangs its rows beneath each Context, and the Sessions read (`sessions`) supplies
 * each row's manager / lead (web/README.md "Context tree rows").
 */
function renderContexts(
  contexts: readonly LiveContext[],
  plans: readonly LivePlan[] = [],
  sessions: readonly TreeSession[] = [],
  onSelectSession: (id: string) => void = () => {},
): void {
  act(() => {
    root.render(
      <SidebarProvider>
        <Tree
          sessions={sessions}
          plans={plans}
          contexts={contexts}
          onSelectSession={onSelectSession}
        />
      </SidebarProvider>,
    );
  });
}

/** Every row: a real <button> carrying its Session's id. */
function rows(): HTMLButtonElement[] {
  return [...container.querySelectorAll<HTMLButtonElement>("button[data-session-id]")];
}

/** Every <button> in the rail, whatever it carries: a Context row, a Plan row or a Session row. */
function buttons(): HTMLButtonElement[] {
  return [...container.querySelectorAll<HTMLButtonElement>("button")];
}

/** The row whose text carries `text`: the Context row (its id) or a Plan row (its key). */
function rowContaining(text: string): HTMLButtonElement | undefined {
  return buttons().find((button) => (button.textContent ?? "").includes(text));
}

/** The Plan rows in the DOM's order: the rail's buttons carrying either Plan's key. */
function planRows(): HTMLButtonElement[] {
  return buttons().filter((button) => {
    const text = button.textContent ?? "";
    return text.includes(PLAN_ALPHA.id) || text.includes(PLAN_BETA.id);
  });
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

/** The Context's fold control: the `aria-expanded` button inside that Context's row (web/README.md "Context tree rows"). */
function foldControlFor(contextId: string): HTMLButtonElement | undefined {
  return [...container.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")].find((button) =>
    (button.closest('[data-slot="sidebar-menu-item"]')?.textContent ?? "").includes(contextId),
  );
}

/** The row's glyph dot: the rounded element inside the row carrying `key`. */
function dotFor(key: string): HTMLElement | undefined {
  const glyph = rowContaining(key)?.querySelector<HTMLElement>('[class*="rounded-full"]');
  return glyph ?? undefined;
}

/** A dot's colour value: its class list plus any inline style, never a pixel. */
function dotColour(dot: HTMLElement | undefined): string {
  return `${dot?.className ?? ""} ${dot?.getAttribute("style") ?? ""}`;
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
  render(SESSIONS.slice(0, 1));
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

// The Plan tree (web/README.md "Context tree rows", hs-d837/hs-d838). Written from the README
// alone: the Context row is glyph + key with no numbers, a Plan row is glyph + key + its nonzero
// counters in the contract's order (`·` apart), and a Plan row selects its Lead Session.

test("a Context row shows its id and no counter digits", () => {
  renderPlans(PLANS, PLAN_SESSIONS, () => {}, CONTEXTS);

  const context = rowContaining(CONTEXT);
  expect(context).toBeDefined();
  const text = context?.textContent ?? "";
  // The Context's id is the row's key.
  expect(text).toContain(CONTEXT);
  // A Context row carries no numbers at all: the counters are drawn on Plan rows only.
  expect(text).not.toMatch(/\d/);
});

test("a Plan row shows its key, its nonzero counters in the contract's order and never a zero", () => {
  renderPlans(PLANS, PLAN_SESSIONS, () => {}, CONTEXTS);

  // Listed beta-first, the Context/Plan-key sort puts `alpha` first.
  const first = planRows()[0];
  expect(first).toBeDefined();
  const text = first?.textContent ?? "";
  expect(text).toContain(PLAN_ALPHA.id);
  // planned = draft + queued = 3, active = running = 1, merged = 4; failed = 0 is dropped. A
  // single dot separates two drawn roles, in the cockpit's order.
  expect(text).toMatch(/3\s*·\s*1\s*·\s*4/);
  expect(text).not.toMatch(/0/);
});

test("a Plan row with no counts shows its key and no number", () => {
  renderPlans(PLANS, PLAN_SESSIONS, () => {}, CONTEXTS);

  const beta = rowContaining(PLAN_BETA.id);
  expect(beta).toBeDefined();
  const text = beta?.textContent ?? "";
  expect(text).toContain(PLAN_BETA.id);
  // Every count is zero, so nothing is drawn: not even a zero.
  expect(text).not.toMatch(/\d/);
});

test("clicking a Plan row reports its Lead Session", () => {
  const clicked: string[] = [];
  renderPlans(PLANS, PLAN_SESSIONS, (id) => clicked.push(id), CONTEXTS);

  const alpha = rowContaining(PLAN_ALPHA.id);
  expect(alpha).toBeDefined();
  if (alpha === undefined) return;
  act(() => {
    alpha.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  expect(clicked).toEqual([LEAD_A]);
});

test("a rail given no plans still renders its Session rows", () => {
  // An API without the plans read passes an empty list; the rail falls back to its Session tree.
  renderPlans([], SESSIONS);

  expect(new Set(ids(rows()))).toEqual(new Set([MA, MB, LA1, LA2]));
});

// The corrected Context tree (web/README.md "Context tree rows", User 2026-11-02 "cockpit
// behaviour"): the rail lists EVERY Context of the Contexts read (not just those with a live
// Plan), an archived Context comes folded, a Context row selects its manager Session and is
// disabled only without one, a Context's archived Plans hide behind its `aria-expanded` fold,
// and the rows' dots are the palette's meanings, never a Tailwind hue.

const CTX_ALONE = "solo-cpd";
const CTX_ARCHIVED = "old-cpd";
const MANAGER = "cccccccc-0000-0000-0000-000000000001";
const LEAD_WORK = "dddddddd-0000-0000-0000-000000000001";
const LEAD_SLEEP = "dddddddd-0000-0000-0000-000000000002";

/** The manager Session of a Context, as the Sessions read carries it. */
function manager(context: string): TreeSession {
  return { id: MANAGER, role: "manager", runner: "durable", context, name: "Manager One" };
}

/** An archived Plan of `context`, listed after the live ones however the read orders them. */
function archivedPlan(context: string, id: string): LivePlan {
  return { ...PLAN_ALPHA, id, context, archived_at: "2026-11-02T00:00:00Z" };
}

test("a Context without any Plan is still listed", () => {
  // Neither a live Plan nor an archived one: the Contexts read alone lists the row.
  renderContexts([{ id: CTX_ALONE, archived: false }], [], []);

  const context = rowContaining(CTX_ALONE);
  expect(context).toBeDefined();
  expect(context?.textContent ?? "").toContain(CTX_ALONE);
});

test("an archived Context starts folded", () => {
  renderContexts(
    [
      { id: CONTEXT, archived: false },
      { id: CTX_ARCHIVED, archived: true },
    ],
    [PLAN_ALPHA, archivedPlan(CTX_ARCHIVED, "arch-plan")],
    PLAN_SESSIONS,
  );

  const fold = foldControlFor(CTX_ARCHIVED);
  expect(fold).toBeDefined();
  // Folded by default: the control reports `aria-expanded` false.
  expect(fold?.getAttribute("aria-expanded")).toBe("false");
});

test("clicking a Context row selects its manager Session", () => {
  const clicked: string[] = [];
  renderContexts([{ id: CONTEXT, archived: false }], [], [manager(CONTEXT)], (id) => clicked.push(id));

  const context = rowContaining(CONTEXT);
  expect(context).toBeDefined();
  if (context === undefined) return;
  act(() => {
    context.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  expect(clicked).toEqual([MANAGER]);
});

test("a Context with no manager Session is disabled", () => {
  const clicked: string[] = [];
  renderContexts(
    [
      { id: CTX_ALONE, archived: false },
      { id: CONTEXT, archived: false },
    ],
    [],
    [manager(CONTEXT)],
    (id) => clicked.push(id),
  );

  const orphan = rowContaining(CTX_ALONE);
  expect(orphan).toBeDefined();
  expect(orphan?.disabled).toBe(true);
  if (orphan !== undefined) {
    act(() => {
      orphan.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
  }
  expect(clicked).toEqual([]);

  // A Context WITH a manager is not disabled: it is never disabled for want of a live Plan.
  const staffed = rowContaining(CONTEXT);
  expect(staffed).toBeDefined();
  expect(staffed?.disabled).toBe(false);
  if (staffed === undefined) return;
  act(() => {
    staffed.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  expect(clicked).toEqual([MANAGER]);
});

test("a Context's archived Plan hides until its fold is opened", () => {
  const live: LivePlan = { ...PLAN_ALPHA, id: "live-plan" };
  const archived = archivedPlan(CONTEXT, "arch-plan");
  renderContexts([{ id: CONTEXT, archived: false }], [live, archived], PLAN_SESSIONS);

  // A live Plan always shows; the archived one hides behind the fold.
  expect(rowContaining(live.id)).toBeDefined();
  expect(rowContaining(archived.id)).toBeUndefined();

  const fold = foldControlFor(CONTEXT);
  expect(fold).toBeDefined();
  expect(fold?.getAttribute("aria-expanded")).toBe("false");
  if (fold === undefined) return;
  act(() => {
    fold.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });

  expect(foldControlFor(CONTEXT)?.getAttribute("aria-expanded")).toBe("true");
  expect(rowContaining(archived.id)).toBeDefined();
  expect(rowContaining(live.id)).toBeDefined();
});

test("an asleep row and a working row take different palette dots", () => {
  renderContexts(
    [{ id: CONTEXT, archived: false }],
    [
      { ...PLAN_ALPHA, id: "work-plan", lead_session: LEAD_WORK },
      { ...PLAN_ALPHA, id: "sleep-plan", lead_session: LEAD_SLEEP },
    ],
    [
      { id: LEAD_WORK, role: "lead", runner: "durable", context: CONTEXT, name: "Working Lead", state: "working" },
      { id: LEAD_SLEEP, role: "lead", runner: "durable", context: CONTEXT, name: "Asleep Lead", state: "sleeping" },
    ],
  );

  const workDot = dotFor("work-plan");
  const sleepDot = dotFor("sleep-plan");
  expect(workDot).toBeDefined();
  expect(sleepDot).toBeDefined();
  // The working dot takes the palette's mizu value; the asleep one is a different value.
  expect(dotColour(workDot)).toMatch(/mizu/);
  expect(dotColour(sleepDot)).not.toBe(dotColour(workDot));
  // No Tailwind hue (emerald, violet, red) is used anywhere in the rail.
  expect(container.innerHTML).not.toMatch(/emerald|violet|(?:bg|text|border|ring|from|to|via)-red-/);
});

test("an asleep lead's row is dim, keyed on asleep_at", () => {
  // The read marks a put-to-sleep lead with `asleep_at` set while its `state` stays `stopped`;
  // `idle` says nothing about sleep. The row is dim keyed on `asleep_at`, whatever the state
  // (web/README.md "Context tree rows").
  const SLEEP_STOPPED = "cccccccc-0000-0000-0000-000000000001";
  const AWAKE_IDLE = "cccccccc-0000-0000-0000-000000000002";
  const SLEEP_IDLE = "cccccccc-0000-0000-0000-000000000003";
  const AWAKE_STOPPED = "cccccccc-0000-0000-0000-000000000004";
  const AWAKE_WORKING = "cccccccc-0000-0000-0000-000000000005";
  const plan = (key: string, lead: string): LivePlan => ({ ...PLAN_ALPHA, id: key, lead_session: lead });
  const ASLEEP_AT = "2026-11-01T00:00:00Z";

  renderContexts(
    [{ id: CONTEXT, archived: false }],
    [
      plan("sleep-stopped", SLEEP_STOPPED),
      plan("awake-idle", AWAKE_IDLE),
      plan("sleep-idle", SLEEP_IDLE),
      plan("awake-stopped", AWAKE_STOPPED),
      plan("awake-working", AWAKE_WORKING),
    ],
    [
      { id: SLEEP_STOPPED, role: "lead", runner: "durable", context: CONTEXT, name: "Sleeping Stopped", state: "stopped", asleep_at: ASLEEP_AT },
      { id: AWAKE_IDLE, role: "lead", runner: "durable", context: CONTEXT, name: "Awake Idle", state: "idle" },
      { id: SLEEP_IDLE, role: "lead", runner: "durable", context: CONTEXT, name: "Sleeping Idle", state: "idle", asleep_at: ASLEEP_AT },
      { id: AWAKE_STOPPED, role: "lead", runner: "durable", context: CONTEXT, name: "Awake Stopped", state: "stopped" },
      { id: AWAKE_WORKING, role: "lead", runner: "durable", context: CONTEXT, name: "Awake Working", state: "working" },
    ],
  );

  // The winning class carries Tailwind's important modifier, so it beats the sidebar component's
  // own `text-sidebar-foreground` on the same element (web/README.md "Context tree rows").
  const DIM = "text-[var(--dango-dim)]!";
  /** The row's counter spans: the numbers and their `·` separators, the container's children. */
  const counterSpans = (key: string): HTMLElement[] => [
    ...(rowContaining(key)?.querySelectorAll<HTMLElement>('[class*="tabular-nums"] > span') ?? []),
  ];

  // Only the rows whose lead carries `asleep_at` take the dim row class, whatever the state says,
  // and every one of their counter spans does too — the `·` separator included.
  for (const key of ["sleep-stopped", "sleep-idle"]) {
    expect(rowContaining(key)?.className).toContain(DIM);
    const spans = counterSpans(key);
    expect(spans.length).toBeGreaterThan(0);
    for (const span of spans) {
      expect(span.className).toContain(DIM);
    }
  }
  // An `asleep_at`-less idle Session carries none of it: neither its button nor its counters.
  for (const key of ["awake-idle", "awake-stopped", "awake-working"]) {
    expect(rowContaining(key)?.className ?? "").not.toContain(DIM);
    for (const span of counterSpans(key)) {
      expect(span.className).not.toContain(DIM);
    }
  }
  // An asleep row does not take the working row's class; its dot is the palette's dim grey and an
  // idle Session's dot is the plain grey (web/README.md "Context tree rows").
  expect(rowContaining("sleep-stopped")?.className).not.toBe(rowContaining("awake-working")?.className);
  expect(dotColour(dotFor("sleep-stopped"))).toMatch(/dango-dim/);
  expect(dotColour(dotFor("awake-idle"))).toMatch(/dango-plain/);
  expect(dotColour(dotFor("awake-working"))).toMatch(/dango-mizu/);
});

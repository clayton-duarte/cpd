// @vitest-environment jsdom
// Tests for the single-screen AI Elements chat reachable from the Context tree in the left rail
// (web/README.md "The Context tree in the left rail selects the Chat"): the Chat is the centre
// column, with the Context rail to its left and the Inbox placeholder to its right. The whole
// screen is the three-column shell (`App`): the left rail is the Chat's only navigation and
// nothing is selected on first render, so every test renders `App` exactly as `main.tsx` does,
// clicks a Session's row in the rail, and pins what the screen then shows and what the injected
// LiveApi receives. The shell's own three columns and its 20rem default are pinned by
// web/src/app/App.test.tsx, not here.
//
// Only authored code is tested. The LiveApi is an injected fake: no EventSource opens, no fetch
// runs, no socket, server or cpd spawns. The frame the view folds is the same `SnapshotFrame`
// the existing fold already reads (fold rule 3).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, expect, test } from "vitest";
import { App } from "../app/App";
import type { LiveApi } from "../api/live";
import type { LiveSession } from "../api/live-server";
import type { SnapshotFrame } from "../fold/fold";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

/** Flushes pending microtasks (the view's awaits). */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
}

/** A fake LiveApi: the sessions it lists, the ids it subscribed to and closed, the sends. */
interface FakeLiveApi {
  api: LiveApi;
  subscribed: string[];
  closed: string[];
  sent: Array<{ id: string; text: string }>;
  emit: (id: string, frame: SnapshotFrame) => void;
}

/** Builds the injected fake; `emit` hands a frame to the live subscription of one Session. */
function fakeLiveApi(sessions: readonly LiveSession[]): FakeLiveApi {
  const subscribed: string[] = [];
  const closed: string[] = [];
  const sent: Array<{ id: string; text: string }> = [];
  const callbacks = new Map<string, (frame: SnapshotFrame) => void>();
  const api: LiveApi = {
    sessions: async () => sessions,
    subscribe: (id, onFrame) => {
      subscribed.push(id);
      callbacks.set(id, onFrame);
      return () => {
        closed.push(id);
        callbacks.delete(id);
      };
    },
    send: async (id, text) => {
      sent.push({ id, text });
    },
  };
  return { api, subscribed, closed, sent, emit: (id, frame) => callbacks.get(id)?.(frame) };
}

/** A lead Session whose tree row is labelled from its `name`; `context` groups it in the rail. */
function lead(id: string, label: string, context: string): LiveSession {
  return { id, role: "lead", runner: "pi", title: label, context, name: label };
}

/** Renders the shell the way `main.tsx` mounts it. */
async function renderScreen(live?: LiveApi): Promise<void> {
  await act(async () => {
    root.render(
      <MemoryRouter>
        <App live={live} />
      </MemoryRouter>,
    );
    await flush();
  });
}

/** The rail row button for a Session: the tree's only navigation. */
function sessionButton(id: string): HTMLButtonElement | null {
  return container.querySelector<HTMLButtonElement>(`button[data-session-id="${id}"]`);
}

/** Clicks a Session's row in the rail, selecting it for the Chat. */
async function selectSession(id: string): Promise<void> {
  const button = sessionButton(id);
  expect(button).not.toBeNull();
  await act(async () => {
    button!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await flush();
  });
}

/** Sets a form control's value the way React observes it, then fires `input` and `change`. */
function setValue(element: HTMLInputElement | HTMLTextAreaElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(element), "value")?.set;
  setter?.call(element, value);
  element.dispatchEvent(new Event("input", { bubbles: true }));
  element.dispatchEvent(new Event("change", { bubbles: true }));
}

/** A snapshot frame carrying a message through the existing fold (rule 3). */
function snapshot(text: string, role: "user" | "assistant" = "assistant"): SnapshotFrame {
  return {
    type: "snapshot",
    epoch: "live",
    seq: 1,
    snapshot: { messages: [{ id: "m1", role, text, pending: false }] },
  };
}

test("the chat is the only element on the screen, with no nav and no other panes", async () => {
  const { api } = fakeLiveApi([lead("s1", "Lead one", "alpha")]);

  await renderScreen(api);

  // The legacy single-screen cockpit chrome stays gone: no nav, no Tree/Plan/Inbox/Logs panes.
  expect(container.querySelector("nav")).toBeNull();
  for (const pane of ["tree", "plan", "inbox", "logs"]) {
    expect(container.querySelector(`[data-view="${pane}"]`)).toBeNull();
  }

  // The Chat is the centre column of the shell: the Context rail is to its left and the Inbox
  // rail to its right. The shell's three columns and 20rem default are App.test.tsx's.
  const chat = container.querySelector("textarea")?.closest<HTMLElement>("[data-panel]");
  expect(chat).not.toBeNull();
  const context = chat!.previousElementSibling?.previousElementSibling as HTMLElement | null;
  expect(context?.querySelector('[data-slot="sidebar"]')).not.toBeNull();
  const inbox = chat!.nextElementSibling?.nextElementSibling as HTMLElement | null;
  expect(inbox?.querySelector('[data-slot="sidebar"]')).not.toBeNull();
  expect(inbox?.textContent).toContain("William Smith");
});

test("the transcript renders inside a scroll container and the composer stays visible", async () => {
  const fake = fakeLiveApi([lead("s1", "Lead one", "alpha")]);

  await renderScreen(fake.api);
  // Nothing is selected on first render: clicking the rail row selects the Session.
  await selectSession("s1");
  await act(async () => {
    fake.emit("s1", snapshot("in the transcript"));
    await flush();
  });
  expect(container.textContent).toContain("in the transcript");

  // The scroll region is the transcript's viewport; the message lives in its log.
  const viewport = container.querySelector('[role="region"]');
  expect(viewport).not.toBeNull();
  const transcript = viewport!.querySelector('[role="log"]');
  expect(transcript).not.toBeNull();
  expect(transcript!.textContent).toContain("in the transcript");

  // The composer is a sibling of the scroll region, so it never scrolls away.
  const composer = container.querySelector("form");
  expect(composer).not.toBeNull();
  expect(viewport!.contains(composer!)).toBe(false);
});

test("an assistant message renders its markdown, never raw text", async () => {
  const fake = fakeLiveApi([lead("s1", "Lead one", "alpha")]);

  await renderScreen(fake.api);
  await selectSession("s1");
  await act(async () => {
    fake.emit("s1", snapshot("# Heading\n\na **bold** word"));
    await flush();
  });

  // The markdown became elements, and the raw source is gone from the screen.
  expect(container.querySelector("h1")).not.toBeNull();
  expect(container.querySelector("strong")).not.toBeNull();
  expect(container.textContent).toContain("bold");
  expect(container.textContent).not.toContain("**bold**");
});

test("the session picker lists the live sessions and switching re-subscribes", async () => {
  const fake = fakeLiveApi([
    lead("s1", "Lead one", "alpha"),
    { id: "s2", role: "manager", runner: "durable", title: "Manager two", context: "alpha", name: "Manager two" },
  ]);

  await renderScreen(fake.api);

  // The rail lists both live Sessions; nothing is selected or subscribed on first render.
  expect(sessionButton("s1")).not.toBeNull();
  expect(sessionButton("s2")).not.toBeNull();
  expect(sessionButton("s1")!.textContent).toContain("Lead one");
  expect(sessionButton("s2")!.textContent).toContain("Manager two");
  expect(fake.subscribed).toEqual([]);

  // Selecting a Session subscribes the Chat to it.
  await selectSession("s1");
  expect(fake.subscribed).toEqual(["s1"]);

  // Selecting another closes the old subscription and subscribes to the new Session.
  await selectSession("s2");
  expect(fake.subscribed).toEqual(["s1", "s2"]);
  expect(fake.closed).toEqual(["s1"]);
});

test("the composer sends through the live API and clears", async () => {
  const fake = fakeLiveApi([lead("s1", "Lead one", "alpha")]);

  await renderScreen(fake.api);
  await selectSession("s1");

  const textarea = container.querySelector("textarea");
  expect(textarea).not.toBeNull();
  await act(async () => {
    setValue(textarea!, "hello there");
    await flush();
  });

  const form = textarea!.closest("form");
  expect(form).not.toBeNull();
  await act(async () => {
    form!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await flush();
  });

  // It left through the live API for the selected Session, and the box cleared.
  expect(fake.sent).toEqual([{ id: "s1", text: "hello there" }]);
  expect(textarea!.value).toBe("");
});

test("with no live adapter the surface shows an explicit unavailable state", async () => {
  await renderScreen();

  expect(container.textContent).toContain("Live adapter off");
  expect(container.textContent).toContain("CPD_WEB_LIVE=1");
});

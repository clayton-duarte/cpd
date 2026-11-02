// @vitest-environment jsdom
// Pins the shell-level rules the Chat column's contract adds: the 2rem bottom padding under the
// message list (web/README.md "The chat surface (shadcn AI Elements)" and hs-d835) so no rendered
// text touches the prompt input, and the page-scroll clamp (web/README.md "The resizable
// three-column shell"): the Chat takes its own column's height, both rail providers are
// height-bounded, and index.css clamps the document so the page itself never scrolls.
//
// Only authored code is tested: the LiveApi is an injected fake, no socket, server or network.
// jsdom has no layout, so the class tokens and the stylesheet's own structure are what the tests
// assert, never a computed box.
import { readFileSync } from "node:fs";
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { LiveChat, type LiveChatProps } from "./live-chat";
import { App } from "../app/App";
import { parsePiSession, toSnapshotFrame } from "../transcript";
import type { LiveApi } from "../api/live";
import type { LiveSession } from "../api/live-server";
import type { SnapshotFrame } from "../fold/fold";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** A Session as the Chat now receives it: `LiveSession` plus the adapter's `context` and `name`. */
type TreeSession = LiveSession & { context: string; name?: string };

/** The Chat's props plus the new `session`; LiveChatProps lacks it on the base branch. */
type LiveChatSessionProps = LiveChatProps & { session?: TreeSession };

const LiveChatWithSession = LiveChat as unknown as (props: LiveChatSessionProps) => ReactElement;

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

/** Flushes pending microtasks (the view's awaits). */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
}

/** A fake LiveApi: one Session it subscribes to, and a frame emitter into that subscription. */
function fakeApi(session: TreeSession): { api: LiveApi; emit: (frame: SnapshotFrame) => void } {
  let onFrame: ((frame: SnapshotFrame) => void) | undefined;
  const api = {
    sessions: async () => [session],
    subscribe: (_id: string, callback: (frame: SnapshotFrame) => void) => {
      onFrame = callback;
      return () => {
        onFrame = undefined;
      };
    },
    send: async () => {},
  } as unknown as LiveApi;
  return { api, emit: (frame) => onFrame?.(frame) };
}

/** The message list's own line and wrapper, for the JSONL fixture. */
function messageLine(id: string, role: "user" | "assistant", content: unknown): string {
  return JSON.stringify({ type: "message", id, message: { role, content } });
}

test("the Chat's message list carries the 2rem bottom padding, so no text touches the prompt", async () => {
  const session: TreeSession = { id: "s1", role: "lead", runner: "pi", context: "ctx", name: "Lead one" };
  const { api, emit } = fakeApi(session);

  await act(async () => {
    root.render(<LiveChatWithSession api={api} token="t0k" session={session} />);
    await flush();
  });
  await act(async () => {
    emit(toSnapshotFrame(parsePiSession(messageLine("a1", "assistant", "the newest turn"))));
    await flush();
  });

  const content = container.querySelector('[data-slot="message-scroller-content"]');
  expect(content).not.toBeNull();
  expect(content?.classList.contains("pb-8")).toBe(true);
});

test("the transcript is inset 16px on both sides", async () => {
  const session: TreeSession = { id: "s1", role: "lead", runner: "pi", context: "ctx", name: "Lead one" };
  const { api, emit } = fakeApi(session);

  await act(async () => {
    root.render(<LiveChatWithSession api={api} token="t0k" session={session} />);
    await flush();
  });
  await act(async () => {
    emit(toSnapshotFrame(parsePiSession(messageLine("a1", "assistant", "the newest turn"))));
    await flush();
  });

  const content = container.querySelector('[data-slot="message-scroller-content"]');
  expect(content).not.toBeNull();
  expect(content?.classList.contains("px-4")).toBe(true);
});

test("the Chat takes its own column's height, never a viewport of its own", async () => {
  await act(async () => {
    root.render(<LiveChat />);
    await flush();
  });

  // The authored root is `flex h-full min-h-0 flex-col`: it fills the column it is given.
  expect(container.querySelector("div.flex.h-full.min-h-0.flex-col")).not.toBeNull();
  // It never carries `h-dvh`: the page does not belong to the Chat.
  expect(container.querySelector(".h-dvh")).toBeNull();
});

test("both rail providers are bounded to their column's height", async () => {
  await act(async () => {
    root.render(<App />);
    await flush();
  });

  const wrappers = [...container.querySelectorAll<HTMLElement>('[data-slot="sidebar-wrapper"]')];
  expect(wrappers).toHaveLength(2);
  for (const wrapper of wrappers) {
    expect(wrapper.classList.contains("h-full")).toBe(true);
    expect(wrapper.classList.contains("min-h-0")).toBe(true);
  }
});

test("index.css clamps the document so the page itself never scrolls", () => {
  const css = readFileSync(`${import.meta.dirname}/../index.css`, "utf8");

  // `html, body, #root` are pinned to the viewport's height...
  const clamp = /html,\s*body,\s*#root\s*\{[^}]*height:\s*100%[^}]*\}/.exec(css)?.[0] ?? "";
  expect(clamp).not.toBe("");
  // ...and the body hides its own overflow, so every scrollable thing is a column's own list.
  expect(/body\s*\{[^}]*overflow:\s*hidden[^}]*\}/.test(css)).toBe(true);
});

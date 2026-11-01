// @vitest-environment jsdom
// Red tests for the live Chat view after the Context tree takes over the rail (web/README.md
// "The Context tree in the left rail selects the Chat"). The Chat no longer lists Sessions or
// renders its `Session` <select>; it takes the Session it is given (prop `session`), subscribes
// to it exactly as before, and with no Session shows its pick-from-the-rail state. The `session`
// prop does not exist on the base branch, so the component is reached through a narrow cast and
// the red is a failed assertion, never a build error.
//
// The live Chat is async and interactive (it awaits `api.sessions()` and carries a submit box),
// so unlike the synchronous views harness this file needs a DOM: jsdom in a per-file comment, and
// the client renderer drives the effects and events. Only authored code is tested; the `LiveApi`
// is an injected fake, so no socket, server or network.
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { LiveChat, type LiveChatProps } from "./live-chat";
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

/** A fake LiveApi: the sessions it lists, the ids it subscribed to, the messages it sent. */
function fakeApi(sessions: readonly TreeSession[]): {
  api: LiveApi;
  subscribed: string[];
  sent: Array<{ id: string; text: string }>;
  sessionsCalls: () => number;
  emit: (frame: SnapshotFrame) => void;
} {
  const subscribed: string[] = [];
  const sent: Array<{ id: string; text: string }> = [];
  let calls = 0;
  let onFrame: ((frame: SnapshotFrame) => void) | undefined;
  const api: LiveApi = {
    sessions: async () => {
      calls += 1;
      return sessions;
    },
    subscribe: (id, callback) => {
      subscribed.push(id);
      onFrame = callback;
      return () => {
        onFrame = undefined;
      };
    },
    send: async (id, text) => {
      sent.push({ id, text });
    },
  };
  return { api, subscribed, sent, sessionsCalls: () => calls, emit: (frame) => onFrame?.(frame) };
}

/** Sets a controlled input's value the way React observes it, then fires `input`. */
function typeInto(element: HTMLInputElement | HTMLTextAreaElement, text: string): void {
  const proto =
    element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")!.set!;
  setter.call(element, text);
  element.dispatchEvent(new Event("input", { bubbles: true }));
}

test("the live Chat renders the Session it is given, with no picker of its own", async () => {
  const session: TreeSession = {
    id: "s1",
    role: "lead",
    runner: "pi",
    title: "Lead one",
    context: "ctx",
    name: "Lead one",
  };
  const { api, subscribed, sessionsCalls, emit } = fakeApi([session]);

  await act(async () => {
    root.render(<LiveChatWithSession api={api} token="t0k" session={session} />);
    await flush();
  });

  // The Chat no longer lists Sessions itself and carries no picker.
  expect(sessionsCalls()).toBe(0);
  expect(container.querySelector("select")).toBeNull();
  // It subscribes to the Session it was given.
  expect(subscribed).toEqual(["s1"]);

  // A snapshot frame folds onto the Chat view and renders that Session's transcript.
  await act(async () => {
    emit({
      type: "snapshot",
      epoch: "live",
      seq: 1,
      snapshot: {
        messages: [{ id: "m1", role: "assistant", text: "given session transcript", pending: false }],
      },
    });
    await flush();
  });
  expect(container.textContent).toContain("given session transcript");
});

test("with no Session the live Chat asks for one from the rail", async () => {
  const { api, sessionsCalls } = fakeApi([]);

  await act(async () => {
    root.render(<LiveChatWithSession api={api} token="t0k" />);
    await flush();
  });

  expect(container.textContent).toContain("Pick a manager or a lead in the left rail to see its chat.");
  expect(container.querySelector("select")).toBeNull();
  expect(sessionsCalls()).toBe(0);
});

test("the live Chat sends a typed message through the live api", async () => {
  const session: TreeSession = {
    id: "s1",
    role: "lead",
    runner: "pi",
    title: "Lead one",
    context: "ctx",
    name: "Lead one",
  };
  const { api, sent } = fakeApi([session]);

  await act(async () => {
    root.render(<LiveChatWithSession api={api} token="t0k" session={session} />);
    await flush();
  });

  const input = container.querySelector<HTMLInputElement | HTMLTextAreaElement>("input, textarea");
  expect(input).not.toBeNull();
  await act(async () => {
    typeInto(input!, "hello there");
  });

  const form = container.querySelector("form");
  expect(form).not.toBeNull();
  await act(async () => {
    form!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await flush();
  });

  expect(sent).toEqual([{ id: "s1", text: "hello there" }]);
});

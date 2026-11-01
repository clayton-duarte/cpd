// @vitest-environment jsdom
// Live Chat view tests for the dev live adapter (web/README.md "The UI wiring", hs-d819). RED:
// `LiveChat` still throws `not implemented`, so these fail now and pass once the ui Job lands.
//
// The live Chat is async and interactive (it awaits `api.sessions()` and carries a submit box),
// so unlike the synchronous views harness this file needs a DOM: jsdom in a per-file comment,
// and the client renderer drives the effects and events. Only authored code is tested; the
// `LiveApi` is an injected fake, so no socket, server or network.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { LiveChat } from "./live-chat";
import type { LiveApi } from "../api/live";
import type { LiveSession } from "../api/live-server";
import type { SnapshotFrame } from "../fold/fold";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
function fakeApi(sessions: LiveSession[]): {
  api: LiveApi;
  subscribed: string[];
  sent: Array<{ id: string; text: string }>;
  emit: (frame: SnapshotFrame) => void;
} {
  const subscribed: string[] = [];
  const sent: Array<{ id: string; text: string }> = [];
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
    send: async (id, text) => {
      sent.push({ id, text });
    },
  };
  return { api, subscribed, sent, emit: (frame) => onFrame?.(frame) };
}

/** Sets a controlled input's value the way React observes it, then fires `input`. */
function typeInto(element: HTMLInputElement | HTMLTextAreaElement, text: string): void {
  const proto =
    element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")!.set!;
  setter.call(element, text);
  element.dispatchEvent(new Event("input", { bubbles: true }));
}

test("the live Chat lists sessions in a picker and renders the selected transcript", async () => {
  const { api, subscribed, emit } = fakeApi([
    { id: "s1", role: "lead", runner: "pi", title: "Lead one" },
    { id: "s2", role: "manager", runner: "durable", title: "Manager two" },
  ]);

  await act(async () => {
    root.render(<LiveChat api={api} token="t0k" />);
    await flush();
  });

  // Both sessions are offered in the picker.
  expect(container.textContent).toContain("Lead one");
  expect(container.textContent).toContain("Manager two");
  // The first session is selected and subscribed to.
  expect(subscribed).toEqual(["s1"]);

  // A snapshot frame folds onto the Chat view and renders the transcript.
  await act(async () => {
    emit({
      type: "snapshot",
      epoch: "live",
      seq: 1,
      snapshot: {
        messages: [{ id: "m1", role: "assistant", text: "hello from the transcript", pending: false }],
      },
    });
    await flush();
  });
  expect(container.textContent).toContain("hello from the transcript");
});

test("the live Chat sends a typed message through the live api", async () => {
  const { api, sent } = fakeApi([{ id: "s1", role: "lead", runner: "pi", title: "Lead one" }]);

  await act(async () => {
    root.render(<LiveChat api={api} token="t0k" />);
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

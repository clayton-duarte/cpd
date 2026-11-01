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

/** How many times `needle` occurs in `haystack` — a reader-visible marking count. */
function occurrences(haystack: string, needle: string): number {
  return needle === "" ? 0 : haystack.split(needle).length - 1;
}

/** The first button whose visible text carries `text`. */
function buttonWithText(text: string): HTMLButtonElement | null {
  return (
    [...container.querySelectorAll("button")].find((button) =>
      (button.textContent ?? "").includes(text),
    ) ?? null
  );
}

/** Renders the Chat for one Session and folds a hand-written pi session JSONL onto it. */
async function renderJsonl(jsonl: string): Promise<void> {
  const session: TreeSession = { id: "s1", role: "lead", runner: "pi", context: "ctx", name: "Lead one" };
  const { api, emit } = fakeApi([session]);
  await act(async () => {
    root.render(<LiveChatWithSession api={api} token="t0k" session={session} />);
    await flush();
  });
  await act(async () => {
    emit(toSnapshotFrame(parsePiSession(jsonl)));
    await flush();
  });
}

/** One assistant text turn line and its wrapper, for the JSONL fixtures. */
function messageLine(id: string, role: "user" | "assistant", content: unknown): string {
  return JSON.stringify({ type: "message", id, message: { role, content } });
}

/** A fake LiveApi whose one paged read answers `page`; records the `before` it was asked for. */
function pagingApi(page: { messages: readonly unknown[]; more: boolean }): {
  api: LiveApi;
  pages: Array<{ id: string; before?: string }>;
  emit: (frame: SnapshotFrame) => void;
} {
  const pages: Array<{ id: string; before?: string }> = [];
  let onFrame: ((frame: SnapshotFrame) => void) | undefined;
  const answer = async (): Promise<{ messages: readonly unknown[]; more: boolean }> => page;
  const api = {
    sessions: async () => [],
    subscribe: (_id: string, callback: (frame: SnapshotFrame) => void) => {
      onFrame = callback;
      return () => {
        onFrame = undefined;
      };
    },
    send: async () => {},
    transcript: async (id: string, requested?: { limit?: number; before?: string }) => {
      pages.push({ id, before: requested?.before });
      return answer();
    },
    history: async (id: string, before?: string) => {
      pages.push({ id, before });
      return answer();
    },
  } as unknown as LiveApi;
  return { api, pages, emit: (frame) => onFrame?.(frame) };
}

test("a person's turn renders as a Bubble and an agent's text as markdown", async () => {
  await renderJsonl(
    [
      messageLine("u1", "user", "**bold words**"),
      messageLine("a1", "assistant", [{ type: "text", text: "an *emphasized* answer" }]),
    ].join("\n"),
  );
  // The person's words render as markdown inside their Bubble.
  expect(container.querySelector("strong")?.textContent).toContain("bold words");
  // The agent's text renders as markdown too.
  expect(container.querySelector("em")?.textContent).toContain("emphasized");
});

test("a thinking part renders as a Trace row carrying its word count", async () => {
  await renderJsonl(
    messageLine("a1", "assistant", [
      { type: "thinking", thinking: "alpha beta gamma delta" },
      { type: "text", text: "the answer" },
    ]),
  );
  expect(container.textContent).toContain("the answer");
  // Four words of thinking, counted on the closed row.
  expect(container.textContent).toMatch(/4\s*words?/i);
});

/** One toolResult line pairing its result onto the call with `toolCallId`. */
function toolResultLine(
  id: string,
  toolCallId: string,
  text: string,
  extra: Record<string, unknown> = {},
): string {
  return JSON.stringify({
    type: "message",
    id,
    message: { role: "toolResult", toolCallId, content: [{ type: "text", text }], ...extra },
  });
}

test("a tool part renders as a Trace row with its title and duration, and opened shows its arguments and result", async () => {
  await renderJsonl(
    [
      messageLine("a1", "assistant", [
        {
          type: "toolCall",
          id: "call-1",
          name: "bash",
          arguments: { command: "npm test -- --run\nsecond line arg" },
        },
      ]),
      toolResultLine("r1", "call-1", "ALL GREEN", { durationMs: 1500, isError: false }),
    ].join("\n"),
  );

  // The title names the command and the duration sits beside it.
  expect(container.textContent).toContain("npm test -- --run");
  expect(container.textContent).toMatch(/1\.5\s*s|1500\s*ms/);

  // Opened, the row shows the call's arguments and its result.
  const trigger = buttonWithText("npm test");
  expect(trigger).not.toBeNull();
  await act(async () => {
    trigger!.click();
    await flush();
  });
  expect(container.textContent).toContain("second line arg");
  expect(container.textContent).toContain("ALL GREEN");
});

test("a failed tool part renders as an error row", async () => {
  await renderJsonl(
    [
      messageLine("a1", "assistant", [
        { type: "toolCall", id: "call-1", name: "read", arguments: { path: "/tmp/missing" } },
      ]),
      toolResultLine("r1", "call-1", "ENOENT: no such file or directory", { isError: true }),
    ].join("\n"),
  );

  // The failed row is in its error state, and its output reads as the failure.
  expect(container.querySelector('[data-status="error"]')).not.toBeNull();
  const trigger = buttonWithText("/tmp/missing");
  expect(trigger).not.toBeNull();
  await act(async () => {
    trigger!.click();
    await flush();
  });
  expect(container.textContent).toContain("ENOENT: no such file or directory");
});

test("a cpd notice renders as a Marker, never as a Bubble", async () => {
  const notice = "cpd engine notice: the Session was archived";
  await renderJsonl(
    [
      JSON.stringify({ type: "message", id: "n1", message: { role: "system", content: notice } }),
      JSON.stringify({ type: "custom_message", id: "n2", display: true, content: notice }),
    ].join("\n"),
  );
  expect(container.textContent).toContain(notice);
});

test("an image part renders inline from its data URL", async () => {
  await renderJsonl(
    messageLine("a1", "assistant", [{ type: "image", data: "aGk=", mimeType: "image/png" }]),
  );
  const image = container.querySelector("img");
  expect(image?.getAttribute("src")).toBe("data:image/png;base64,aGk=");
});

test("Load earlier messages asks for the page above the oldest turn held and disappears when none precedes it", async () => {
  const session: TreeSession = { id: "s1", role: "lead", runner: "pi", context: "ctx", name: "Lead one" };
  const { api, pages, emit } = pagingApi({ messages: [], more: false });

  await act(async () => {
    root.render(<LiveChatWithSession api={api} token="t0k" session={session} />);
    await flush();
  });
  await act(async () => {
    emit(
      toSnapshotFrame(
        parsePiSession(
          [
            messageLine("m1", "assistant", [{ type: "text", text: "the first turn" }]),
            messageLine("m2", "assistant", [{ type: "text", text: "the second turn" }]),
          ].join("\n"),
        ),
      ),
    );
    await flush();
  });

  const control = buttonWithText("Load earlier messages");
  expect(control).not.toBeNull();

  await act(async () => {
    control!.click();
    await flush();
  });

  // It asks for the page above the oldest turn it holds, and hides once none precedes it.
  expect(pages).toHaveLength(1);
  expect(pages[0]!.before).toBe("m1");
  expect(buttonWithText("Load earlier messages")).toBeNull();
});

test("an ask call renders as a question card: its header, its question and every option, the taken one marked", async () => {
  await renderJsonl(
    [
      messageLine("a1", "assistant", [
        {
          type: "toolCall",
          id: "call-1",
          name: "ask_user_question",
          arguments: {
            questions: [
              {
                header: "Deploy",
                question: "Ship it now?",
                options: [
                  { label: "Ship now", description: "release today" },
                  { label: "Hold off", description: "wait a day" },
                ],
              },
            ],
          },
        },
      ]),
      toolResultLine("r1", "call-1", "User has answered your questions", {
        details: {
          answers: [{ questionIndex: 0, question: "Ship it now?", kind: "option", answer: "Ship now" }],
          cancelled: false,
        },
      }),
    ].join("\n"),
  );

  const text = container.textContent ?? "";
  expect(text).toContain("Deploy");
  expect(text).toContain("Ship it now?");
  expect(text).toContain("release today");
  expect(text).toContain("wait a day");
  // The taken option is marked: its label is named again as the outcome, the other is not.
  expect(occurrences(text, "Ship now")).toBeGreaterThanOrEqual(2);
  expect(occurrences(text, "Hold off")).toBe(1);
});

test("a typed ask answer renders as the person's words and a multi answer marks every picked label", async () => {
  await renderJsonl(
    [
      messageLine("a1", "assistant", [
        {
          type: "toolCall",
          id: "call-1",
          name: "ask_user_question",
          arguments: {
            questions: [
              { header: "Notes", question: "Why now?", options: [{ label: "Because", description: "say why" }] },
            ],
          },
        },
      ]),
      toolResultLine("r1", "call-1", "User has answered your questions", {
        details: {
          answers: [{ questionIndex: 0, question: "Why now?", kind: "custom", answer: "please do both" }],
          cancelled: false,
        },
      }),
      messageLine("a2", "assistant", [
        {
          type: "toolCall",
          id: "call-2",
          name: "ask_user_question",
          arguments: {
            questions: [
              {
                header: "Scope",
                question: "Which parts?",
                multiSelect: true,
                options: [
                  { label: "Alpha", description: "the first" },
                  { label: "Beta", description: "the second" },
                  { label: "Gamma", description: "the third" },
                ],
              },
            ],
          },
        },
      ]),
      toolResultLine("r2", "call-2", "User has answered your questions", {
        details: {
          answers: [
            { questionIndex: 0, question: "Which parts?", kind: "multi", answer: null, selected: ["Alpha", "Beta"] },
          ],
          cancelled: false,
        },
      }),
    ].join("\n"),
  );

  const text = container.textContent ?? "";
  // The typed answer reads as the person wrote it.
  expect(text).toContain("please do both");
  expect(text).toContain("Typed answer");
  // The multi answer names its count and every picked label.
  expect(text).toContain("2 of 3 picked");
  expect(text).toContain("Alpha");
  expect(text).toContain("Beta");
});

test("an unanswered ask question says so and a refused ask shows the refusal", async () => {
  await renderJsonl(
    [
      messageLine("a1", "assistant", [
        {
          type: "toolCall",
          id: "call-1",
          name: "ask_user_question",
          arguments: {
            questions: [{ header: "Later", question: "Do it later?", options: [{ label: "Maybe", description: "soon" }] }],
          },
        },
      ]),
      toolResultLine("r1", "call-1", "User has answered your questions", {
        details: {
          answers: [{ questionIndex: 0, question: "Do it later?", kind: "option", answer: null }],
          cancelled: false,
        },
      }),
      messageLine("a2", "assistant", [
        {
          type: "toolCall",
          id: "call-2",
          name: "ask_user_question",
          arguments: {
            questions: [{ header: "Risk", question: "Accept the risk?", options: [{ label: "Accept", description: "go on" }] }],
          },
        },
      ]),
      toolResultLine("r2", "call-2", "User declined to answer questions", {
        details: { answers: [], cancelled: true },
        isError: true,
      }),
    ].join("\n"),
  );

  const text = container.textContent ?? "";
  expect(text).toContain("No answer");
  expect(text).toContain("Refused");
  expect(text).toContain("User declined to answer questions");
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

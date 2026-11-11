// @vitest-environment jsdom
import { act } from "react";
import { describe, expect, it } from "vitest";
import type { TranscriptItem } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { SessionStream, type SessionStreamProps } from "./SessionStream.tsx";

describe("SessionStream", () => {
  it("renders streamed session transcript", () => {
    let subscriber: ((item: TranscriptItem) => void) | null = null;
    const fakeSource = {
      subscribe: (cb: (item: TranscriptItem) => void) => {
        subscriber = cb;
        return () => {
          subscriber = null;
        };
      },
    };

    const m = mount(
      <SessionStream sessionId="session-test-1" source={fakeSource} />,
    );

    expect(m.container.querySelector('[data-part="session-stream"]')).not.toBeNull();

    // Stream a user message
    act(() => {
      subscriber?.({
        kind: "user",
        id: "u1",
        text: "Please inspect core ws connection",
        attachments: [],
      });
    });

    const userEl = m.container.querySelector('[data-kind="user"]');
    expect(userEl).not.toBeNull();
    expect(userEl?.textContent).toContain("Please inspect core ws connection");

    // Stream an assistant response
    act(() => {
      subscriber?.({
        kind: "assistant",
        id: "a1",
        streaming: false,
        blocks: [
          { type: "text", text: "Connected and streaming active events." },
        ],
      });
    });

    const assistantEl = m.container.querySelector('[data-kind="assistant"]');
    expect(assistantEl).not.toBeNull();
    expect(assistantEl?.textContent).toContain("Connected and streaming active events.");

    m.unmount();
  });
});

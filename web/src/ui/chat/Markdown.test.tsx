// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import "../../testing/render.tsx";

const calls: string[] = [];
vi.mock("react-markdown", () => ({
  default: (props: { children: string }) => {
    calls.push(props.children);
    return <p>{props.children}</p>;
  },
}));
vi.mock("remark-gfm", () => ({ default: () => undefined }));

import { Markdown, splitMarkdown } from "./Markdown.tsx";

let mounted: { unmount(): void } | null = null;
beforeEach(() => { calls.length = 0; });
afterEach(() => { mounted?.unmount(); mounted = null; });

describe("splitMarkdown", () => {
  it("splitMarkdown splits paragraphs on blank lines", () => {
    expect(splitMarkdown("one\n\ntwo\n\n\nthree")).toEqual(["one", "two", "three"]);
  });

  it("splitMarkdown keeps a fenced code block with blank lines in one segment", () => {
    const fence = "```js\nlet a;\n\nlet b;\n```";
    expect(splitMarkdown(`intro\n\n${fence}\n\noutro`)).toEqual(["intro", fence, "outro"]);
  });

  it("splitMarkdown keeps an unclosed fence as the last segment", () => {
    const open = "```js\nlet a;\n\nlet b;";
    expect(splitMarkdown(`intro\n\n${open}`)).toEqual(["intro", open]);
  });

  it("splitMarkdown of text without a blank line is one segment", () => {
    expect(splitMarkdown("a\nb\nc")).toEqual(["a\nb\nc"]);
  });
});

describe("Markdown", () => {
  it("Markdown re-renders only the segment that changed", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    mounted = { unmount() { act(() => root.unmount()); container.remove(); } };
    act(() => root.render(<Markdown text={"first\n\nsecond\n\nthird par"} />));
    expect([...calls].sort()).toEqual(["first", "second", "third par"]);
    calls.length = 0;
    act(() => root.render(<Markdown text={"first\n\nsecond\n\nthird partial"} />));
    expect(calls).toEqual(["third partial"]);
  });
});

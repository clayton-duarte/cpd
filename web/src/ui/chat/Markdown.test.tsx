// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { readFileSync } from "node:fs";
const typesetCss = readFileSync(new URL("../../lib/typeset.css", import.meta.url), "utf8");
import { createRoot } from "react-dom/client";
import type { Components } from "react-markdown";
import "../../testing/render.tsx";

const calls: string[] = [];
let capturedComponents: Components | undefined;

vi.mock("react-markdown", () => ({
  default: (props: { children: string; components?: Components }) => {
    calls.push(props.children);
    capturedComponents = props.components;
    if (props.children.startsWith("| Product |")) {
      const rows = props.children.split("\n").filter((_, index) => index !== 1);
      const cells = rows.map((row) => row.split("|").slice(1, -1).map((cell) => cell.trim()));
      const tableChildren = <><thead><tr>{cells[0]!.map((cell) => <th key={cell}>{cell}</th>)}</tr></thead><tbody>{cells.slice(1).map((row) => <tr key={row[0]}>{row.map((cell) => <td key={cell}>{cell}</td>)}</tr>)}</tbody></>;
      return createElement(props.components?.table ?? "table", {}, tableChildren);
    }
    return <p>{props.children}</p>;
  },
}));
vi.mock("remark-gfm", () => ({ default: () => undefined }));

import { Markdown, splitMarkdown } from "./Markdown.tsx";

let mounted: { unmount(): void } | null = null;
beforeEach(() => {
  calls.length = 0;
  capturedComponents = undefined;
});
afterEach(() => {
  mounted?.unmount();
  mounted = null;
});

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
  it("keeps wide Markdown tables accessible in a scrollable region", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    mounted = { unmount() { act(() => root.unmount()); container.remove(); } };
    const markdown = "| Product | Description | Quantity |\n| --- | --- | --- |\n| Alpha | A very long description for a wide table | 12345 |\n| Beta | Another long description for a wide table | 67890 |";
    act(() => root.render(<Markdown text={markdown} />));

    const table = container.querySelector("table");
    expect(table).not.toBeNull();
    expect(Array.from(table!.querySelectorAll("tr"), (row) =>
      Array.from(row.querySelectorAll("th, td"), (cell) => cell.textContent),
    )).toEqual([
      ["Product", "Description", "Quantity"],
      ["Alpha", "A very long description for a wide table", "12345"],
      ["Beta", "Another long description for a wide table", "67890"],
    ]);
    expect(table!.closest(".typeset.typeset-chat")).not.toBeNull();
    const region = table!.closest<HTMLElement>(".typeset-scroll");
    expect(region, "table must be inside the existing scroll wrapper").not.toBeNull();
    expect(region!.getAttribute("role")).toBe("region");
    expect(region!.getAttribute("aria-label")?.trim()).toBeTruthy();
    expect(region!.tabIndex).toBe(0);
    // CSS contract only: jsdom does not provide evidence of browser geometry.
    expect(typesetCss).toMatch(/&:where\(\.typeset-scroll\)\s*\{[^}]*overflow-x:\s*auto/s);
    expect(typesetCss).toMatch(/&:where\(\.typeset-scroll table\)\s*\{[^}]*width:\s*max-content;[^}]*max-width:\s*none/s);
  });
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

  it("renders markdown links in new tab with noopener noreferrer", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    mounted = { unmount() { act(() => root.unmount()); container.remove(); } };
    act(() => root.render(<Markdown text="[External](https://example.com/docs)" />));

    expect(capturedComponents?.a, "Markdown components must override 'a' with ExternalLink").toBeDefined();

    const AnchorComponent = capturedComponents!.a as (props: any) => React.ReactElement;
    const linkRoot = document.createElement("div");
    document.body.appendChild(linkRoot);
    const linkReactRoot = createRoot(linkRoot);
    try {
      act(() => {
        linkReactRoot.render(
          <AnchorComponent href="https://example.com/docs">External</AnchorComponent>,
        );
      });
      const a = linkRoot.querySelector("a");
      expect(a).not.toBeNull();
      expect(a?.getAttribute("target")).toBe("_blank");
      expect(a?.getAttribute("rel")).toBe("noopener noreferrer");
    } finally {
      act(() => linkReactRoot.unmount());
      linkRoot.remove();
    }
  });
});

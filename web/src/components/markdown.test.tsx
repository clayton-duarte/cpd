// @vitest-environment jsdom
// Red tests for the markdown a Chat turn renders (web/README.md, "The markdown a turn renders").
// The contract: a turn's text is CommonMark plus GFM, so a table is a real <table> (styled and put
// in its own scroller so it never widens the turn), a task list has checkboxes, a strikethrough is
// a <del>, a bare URL is a link, and a fenced block renders through the Code Block with its
// language as the title. GFM does not change CommonMark's own behaviour: inline code stays inline
// and a single newline in a paragraph is not a line break.
//
// The file renders the component the way the repo's DOM tests do: jsdom in a per-file comment,
// `IS_REACT_ACT_ENVIRONMENT = true`, `createRoot` into a container appended to `document.body`,
// `act` around the render, and an `afterEach` that unmounts and removes the container (the shape
// of web/src/views/live-chat.test.tsx). Only the element and the text are asserted, never a class
// name and never a snapshot.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { Markdown } from "./markdown";

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

/** Renders one literal markdown string through the component and returns the mounted DOM. */
function renderMarkdown(markdown: string): HTMLDivElement {
  act(() => {
    root.render(<Markdown>{markdown}</Markdown>);
  });
  return container;
}

test("a markdown table renders as a table with a header row and its cells", () => {
  renderMarkdown(
    ["| Name | Value |", "| --- | --- |", "| alpha | 1 |", "| beta | 2 |"].join("\n"),
  );

  const table = container.querySelector("table");
  expect(table).not.toBeNull();

  // The header row is a header row, with its cells' text.
  const headers = [...table!.querySelectorAll("thead th")].map((cell) => cell.textContent);
  expect(headers).toEqual(["Name", "Value"]);

  // Every body row renders whole, cell by cell.
  const rows = [...table!.querySelectorAll("tbody tr")];
  expect(rows).toHaveLength(2);
  expect([...rows[0]!.querySelectorAll("td")].map((cell) => cell.textContent)).toEqual([
    "alpha",
    "1",
  ]);
  expect([...rows[1]!.querySelectorAll("td")].map((cell) => cell.textContent)).toEqual([
    "beta",
    "2",
  ]);
});

test("a markdown table is wrapped in its own scroller element, not left to widen the turn", () => {
  renderMarkdown(["| Name | Value |", "| --- | --- |", "| alpha | 1 |"].join("\n"));

  const table = container.querySelector("table");
  expect(table).not.toBeNull();

  // The table is not a direct child of the turn's markdown root: an element of its own wraps it,
  // so the turn's width is the wrapper's and the table scrolls inside instead of widening it.
  const wrapper = table!.parentElement;
  expect(wrapper).not.toBeNull();
  expect(wrapper).not.toBe(container.firstElementChild);
  expect(wrapper!.querySelectorAll("table")).toHaveLength(1);
  // The wrapper holds the table alone; nothing else shares the box.
  expect(wrapper!.textContent).toBe(table!.textContent);
});

test("a task list renders its checkboxes and a strikethrough renders as del", () => {
  renderMarkdown(
    [
      "- [ ] not done",
      "- [x] done",
      "",
      "~~struck out~~",
    ].join("\n"),
  );

  const checkboxes = [...container.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')];
  expect(checkboxes).toHaveLength(2);
  // The checked item is the checked box; the unchecked one is not.
  expect(checkboxes.filter((box) => box.checked)).toHaveLength(1);

  const del = container.querySelector("del");
  expect(del).not.toBeNull();
  expect(del!.textContent).toBe("struck out");
});

test("a bare URL becomes a link and a fenced block becomes a Code Block with its language", () => {
  renderMarkdown(
    ["See https://example.com/page for more.", "", "```ts", "const answer = 42;", "```"].join(
      "\n",
    ),
  );

  // The bare URL is an anchor whose href is the URL and whose text is the URL.
  const anchor = container.querySelector('a[href="https://example.com/page"]');
  expect(anchor).not.toBeNull();
  expect(anchor!.textContent).toBe("https://example.com/page");

  // The fenced block renders through the Code Block, with its language as the title.
  const block = container.querySelector('[data-slot="code-block"]');
  expect(block).not.toBeNull();
  expect(block!.querySelector('[data-slot="code-block-title"]')?.textContent).toBe("ts");
  expect(block!.textContent).toContain("const answer = 42;");
});

test("inline code stays inline and a single newline in a paragraph is not a line break", () => {
  renderMarkdown("inline `code` here\nand the next line");

  // Inline code stays a `code` element, never a fenced block.
  const code = container.querySelector("code");
  expect(code).not.toBeNull();
  expect(code!.textContent).toBe("code");
  expect(container.querySelector("pre")).toBeNull();
  expect(container.querySelector('[data-slot="code-block"]')).toBeNull();

  // A single newline inside a paragraph is a soft break, not a `br`.
  expect(container.querySelector("br")).toBeNull();
  expect(container.textContent).toContain("here");
  expect(container.textContent).toContain("and the next line");
});

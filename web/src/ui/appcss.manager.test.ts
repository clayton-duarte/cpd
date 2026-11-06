import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

const css = readFileSync(new URL("./app.css", import.meta.url), "utf8");

function rule(selector: string): string | null {
  const i = css.indexOf(selector);
  if (i < 0) return null;
  const open = css.indexOf("{", i);
  const close = css.indexOf("}", open);
  return css.slice(open + 1, close);
}

it("app.css hides the manager body and scrollbars when the Plan pane is collapsed", () => {
  const pane = '[aria-label="Plan pane"][data-state="collapsed"]';
  const body = rule(`${pane} [data-part="manager-body"]`);
  expect(body).not.toBeNull();
  expect(body).toMatch(/display:\s*none/);
  const bar = rule(`${pane} [data-slot="scroll-area-scrollbar"]`);
  expect(bar).not.toBeNull();
  expect(bar).toMatch(/display:\s*none/);
  const header = rule(`${pane} [data-part="manager-header"]`);
  expect(header).not.toBeNull();
  expect(header).toMatch(/border-bottom-color:\s*transparent/);
});

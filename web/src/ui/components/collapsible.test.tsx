// @vitest-environment jsdom
import { expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { Collapsible, CollapsibleContent } from "./collapsible.tsx";

it("CollapsibleContent carries the animate-collapsible-down and animate-collapsible-up classes", () => {
  const m = mount(
    <Collapsible open>
      <CollapsibleContent>body</CollapsibleContent>
    </Collapsible>,
  );
  const el = m.container.querySelector("[data-slot=collapsible-content]");
  expect(el, "content element").not.toBeNull();
  const cls = el?.className ?? "";
  expect(cls, "open animation").toContain("animate-collapsible-down");
  expect(cls, "closed animation").toContain("animate-collapsible-up");
  m.unmount();
});

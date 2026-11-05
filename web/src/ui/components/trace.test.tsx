// @vitest-environment jsdom
import { expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { TraceCollapsible, TraceCollapsibleTrigger, TraceDescription } from "./trace.tsx";

it("trace chevron rotates with a 200ms transition", () => {
  const m = mount(
    <TraceCollapsible>
      <TraceCollapsibleTrigger>title</TraceCollapsibleTrigger>
    </TraceCollapsible>,
  );
  const icon = m.container.querySelector("[data-slot=trace-collapsible-trigger] > [data-slot=trace-icon]");
  expect(icon, "chevron icon").not.toBeNull();
  const tokens = (icon?.className ?? "").split(/\s+/);
  expect(tokens, "transition").toContain("transition-transform");
  expect(tokens, "duration").toContain("duration-200");
  m.unmount();
});

it("TraceDescription has no max-w-[45%] class", () => {
  const m = mount(<TraceDescription>desc</TraceDescription>);
  const el = m.container.querySelector("[data-slot=trace-description]");
  expect(el, "description").not.toBeNull();
  expect(el?.classList.contains("max-w-[45%]")).toBe(false);
  m.unmount();
});

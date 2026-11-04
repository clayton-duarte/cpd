// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "./resizable.tsx";

// jsdom has no ResizeObserver, which react-resizable-panels (external) needs: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

it("resizable parts render their documented hooks", () => {
  const m = mount(
    <ResizablePanelGroup id="g" orientation="horizontal">
      <ResizablePanel id="a"><i id="a1">A</i></ResizablePanel>
      <ResizableHandle id="h" withHandle aria-label="Resize a and b" />
      <ResizablePanel id="b"><i id="b1">B</i></ResizablePanel>
    </ResizablePanelGroup>,
  );
  const c = m.container;
  const group = c.querySelector('[data-slot="resizable-panel-group"]');
  expect(group, "group slot").not.toBeNull();
  expect(group?.querySelectorAll('[data-slot="resizable-panel"]').length, "panel slots").toBe(2);
  expect(group?.querySelector('[data-slot="resizable-panel"] #a1'), "panel children").not.toBeNull();
  const handle = c.querySelector('[data-slot="resizable-handle"]');
  expect(handle?.getAttribute("role"), "handle role").toBe("separator");
  expect(handle?.getAttribute("aria-label"), "handle label").toBe("Resize a and b");
  expect(handle?.getAttribute("tabindex"), "handle focusable").toBe("0");
  expect(handle?.querySelector("div"), "withHandle grip").not.toBeNull();
  m.unmount();
});

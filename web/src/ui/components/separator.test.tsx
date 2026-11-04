// @vitest-environment jsdom
import { expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { ScrollArea } from "./scroll-area.tsx";
import { Separator } from "./separator.tsx";

it("separator and scroll area render their documented hooks", () => {
  const m = mount(
    <div>
      <Separator />
      <Separator orientation="vertical" />
      <ScrollArea><i id="kid">kid</i></ScrollArea>
    </div>,
  );
  const seps = m.container.querySelectorAll('[data-slot="separator"]');
  expect(seps, "separator slots").toHaveLength(2);
  expect(seps[0]?.getAttribute("data-orientation"), "default orientation").toBe("horizontal");
  expect(seps[1]?.getAttribute("data-orientation"), "vertical orientation").toBe("vertical");
  const area = m.container.querySelector('[data-slot="scroll-area"]');
  expect(area, "scroll area slot").not.toBeNull();
  expect(area?.querySelector("#kid"), "scroll area children").not.toBeNull();
  m.unmount();
});

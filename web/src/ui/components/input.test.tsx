// @vitest-environment jsdom
import { expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { Input } from "./input.tsx";

it("input renders its documented hooks", () => {
  const m = mount(<Input type="text" className="extra" aria-invalid placeholder="Title:" defaultValue="x" />);
  const el = m.container.querySelector("input");
  expect(el, "input element").not.toBeNull();
  expect(el?.getAttribute("data-slot"), "data-slot").toBe("input");
  expect(el?.getAttribute("type"), "type passes through").toBe("text");
  expect(el?.className, "className is merged").toContain("extra");
  expect(el?.className, "base classes").toContain("border-input");
  expect(el?.getAttribute("aria-invalid"), "aria-invalid passes through").toBe("true");
  expect(el?.getAttribute("placeholder"), "placeholder").toBe("Title:");
  expect(el?.value, "value").toBe("x");
  m.unmount();
});

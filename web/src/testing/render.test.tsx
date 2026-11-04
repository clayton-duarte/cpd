// @vitest-environment jsdom
import { expect, it } from "vitest";
import { mount } from "./render.tsx";

it("mount renders the element into a container", () => {
  const m = mount(<p id="x">hello</p>);
  expect(m.container.querySelector("#x")?.textContent).toBe("hello");
  m.unmount();
});

it("unmount removes the rendered tree", () => {
  const m = mount(<p id="x">hello</p>);
  m.unmount();
  expect(m.container.querySelector("#x")).toBeNull();
});

it("unmount is idempotent", () => {
  const m = mount(<p>hello</p>);
  m.unmount();
  expect(() => m.unmount()).not.toThrow();
});
